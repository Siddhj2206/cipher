//! Sign in with ChatGPT the way Codex does (ADR 0006): OAuth with PKCE and a loopback
//! redirect under Codex's public client, then the Responses API on the ChatGPT backend. It
//! works on every plan Codex works on, Free included, and spends the plan's Codex limits.
//!
//! Source, checked 2026-11-15: `openai/codex` at rust-v0.169.0 (`codex-rs/login/src/server.rs`,
//! `…/oauth/client.rs`, `…/auth/manager.rs`, `codex-rs/codex-api/src/api_bridge.rs`,
//! `…/endpoint/models.rs`), and `numman-ali/opencode-openai-codex-auth` for the request
//! headers.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{LlmError, Secrets};

/// Where the sign-in and the plan's API live. [`Issuer::openai`] in the app; tests point it
/// at stubs.
#[derive(Debug, Clone)]
pub struct Issuer {
    pub issuer: String,
    pub authorize: String,
    pub token: String,
    /// `None` skips the ID-token signature check. Only tests do that.
    pub jwks: Option<String>,
    pub client_id: String,
    /// The Codex backend: `{backend}/responses` and `{backend}/models`.
    pub backend: String,
    /// Loopback ports the client accepts redirects on, in order of preference.
    pub ports: Vec<u16>,
}

impl Issuer {
    pub fn openai() -> Issuer {
        Issuer {
            issuer: "https://auth.openai.com".into(),
            authorize: "https://auth.openai.com/oauth/authorize".into(),
            token: "https://auth.openai.com/oauth/token".into(),
            jwks: Some("https://auth.openai.com/.well-known/jwks.json".into()),
            client_id: "app_EMoamEEZ73f0CkXaXp7hrann".into(),
            backend: "https://chatgpt.com/backend-api/codex".into(),
            // Only these are registered for the client; any other port is refused.
            ports: vec![1455, 1457],
        }
    }
}

const SCOPE: &str = "openid profile email offline_access";
/// Sent as the `originator` query parameter and header, as Codex does.
pub(crate) const ORIGINATOR: &str = "codex_cli_rs";
/// The Codex version the model list is asked for; newer models need a recent one.
pub(crate) const CLIENT_VERSION: &str = "0.169.0";
const CALLBACK_PATH: &str = "/auth/callback";
/// The ID token's claim holding the ChatGPT account and plan.
const AUTH_CLAIM: &str = "https://api.openai.com/auth";
/// Refresh this long before the access token expires.
const REFRESH_MARGIN: i64 = 120;

/// What a sign-in leaves behind. Stored as JSON in [`Secrets`], never in a Book.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    /// The ChatGPT account (`chatgpt_account_id`), sent with every request.
    pub account: String,
    pub email: Option<String>,
    /// `free`, `plus`, `pro`, `team`…, as the ID token names it.
    #[serde(default)]
    pub plan: Option<String>,
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
    pub id_token: String,
}

impl Credentials {
    /// "a@b.c · Free plan", for the Settings line.
    pub fn describe(&self) -> String {
        let who = self.email.clone().unwrap_or_else(|| "ChatGPT".into());
        match self.plan.as_deref().filter(|p| !p.is_empty()) {
            Some(plan) => {
                let mut c = plan.chars();
                let plan: String = c
                    .next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default();
                format!("{who} · {plan} plan")
            }
            None => who,
        }
    }
}

/// Why signing in failed. Every variant is something to show the user.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SignInError {
    #[error("sign-in was cancelled in the browser")]
    Denied,
    #[error("sign-in timed out")]
    TimedOut,
    #[error("sign-in failed: {0}")]
    Failed(String),
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn random_token() -> Result<String, SignInError> {
    let mut buf = vec![0u8; 32];
    SystemRandom::new()
        .fill(&mut buf)
        .map_err(|_| SignInError::Failed("no system randomness".into()))?;
    Ok(B64.encode(buf))
}

/// RFC 7636 S256: base64url(SHA-256(verifier)), unpadded.
fn code_challenge(verifier: &str) -> String {
    B64.encode(ring::digest::digest(
        &ring::digest::SHA256,
        verifier.as_bytes(),
    ))
}

/// A sign-in in progress: the loopback listener is up and [`SignIn::url`] is ready to open
/// in the system browser. Then call [`SignIn::finish`].
pub struct SignIn {
    issuer: Issuer,
    listener: TcpListener,
    redirect_uri: String,
    url: String,
    state: String,
    verifier: String,
}

impl SignIn {
    /// Starts the listener and builds the authorize URL. `email` pre-selects the account
    /// of an earlier sign-in.
    pub fn start(issuer: Issuer, email: Option<&str>) -> Result<SignIn, SignInError> {
        let listener = issuer
            .ports
            .iter()
            .find_map(|p| TcpListener::bind(("127.0.0.1", *p)).ok())
            .ok_or_else(|| {
                SignInError::Failed(format!(
                    "port {} is in use; another sign-in (Codex?) may be waiting in the browser",
                    issuer.ports.first().copied().unwrap_or_default()
                ))
            })?;
        let port = listener
            .local_addr()
            .map_err(|e| SignInError::Failed(e.to_string()))?
            .port();
        let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
        let (state, verifier) = (random_token()?, random_token()?);

        let mut params: Vec<(&str, String)> = vec![
            ("response_type", "code".into()),
            ("client_id", issuer.client_id.clone()),
            ("redirect_uri", redirect_uri.clone()),
            ("scope", SCOPE.into()),
            ("code_challenge", code_challenge(&verifier)),
            ("code_challenge_method", "S256".into()),
            ("id_token_add_organizations", "true".into()),
            ("codex_cli_simplified_flow", "true".into()),
            ("state", state.clone()),
            ("originator", ORIGINATOR.into()),
        ];
        if let Some(email) = email {
            params.push(("login_hint", email.into()));
        }
        let query: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{k}={}", encode(v)))
            .collect();
        let url = format!("{}?{}", issuer.authorize, query.join("&"));
        Ok(SignIn {
            issuer,
            listener,
            redirect_uri,
            url,
            state,
            verifier,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Waits for the browser to come back, exchanges the code and reads the account from
    /// the verified ID token.
    pub fn finish(self, timeout: Duration) -> Result<Credentials, SignInError> {
        let callback = self.wait_for_callback(timeout)?;
        let get = |k: &str| {
            callback
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
        };
        if get("state").as_deref() != Some(self.state.as_str()) {
            return Err(SignInError::Failed(
                "the callback's state did not match".into(),
            ));
        }
        if let Some(err) = get("error") {
            return Err(if err == "access_denied" {
                SignInError::Denied
            } else {
                SignInError::Failed(match get("error_description") {
                    Some(d) => format!("{err}: {d}"),
                    None => err,
                })
            });
        }
        let code =
            get("code").ok_or_else(|| SignInError::Failed("no code in the callback".into()))?;

        let tokens = post_form(
            &self.issuer.token,
            &[
                ("grant_type", "authorization_code"),
                ("client_id", &self.issuer.client_id),
                ("code", &code),
                ("redirect_uri", &self.redirect_uri),
                ("code_verifier", &self.verifier),
            ],
        )
        .map_err(|e| SignInError::Failed(e.describe()))?;

        let id_token = str_field(&tokens, "id_token")
            .ok_or_else(|| SignInError::Failed("no ID token in the response".into()))?;
        let claims = verify_id_token(&self.issuer, &id_token)?;
        let aud = audience(&claims);
        if !aud.is_empty() && !aud.contains(&self.issuer.client_id) {
            return Err(SignInError::Failed(
                "the ID token is for another client".into(),
            ));
        }
        credentials(&tokens, &claims, id_token, None).map_err(|e| SignInError::Failed(e.into()))
    }

    /// Accepts loopback connections until the callback arrives, answering each with a page.
    fn wait_for_callback(&self, timeout: Duration) -> Result<Vec<(String, String)>, SignInError> {
        let deadline = Instant::now() + timeout;
        self.listener
            .set_nonblocking(true)
            .map_err(|e| SignInError::Failed(e.to_string()))?;
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(params) = handle_request(stream) {
                        return Ok(params);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return Err(SignInError::TimedOut);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(SignInError::Failed(e.to_string())),
            }
        }
    }
}

/// What the browser shows after the redirect.
const CALLBACK_PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><meta name="color-scheme" content="light dark"><title>Cipher</title><style>body{margin:0;min-height:100vh;display:grid;place-items:center;font:16px/1.5 system-ui,sans-serif;background:#f6f3ee;color:#2b2722}@media(prefers-color-scheme:dark){body{background:#1c1a17;color:#e9e4dc}}main{text-align:center;max-width:28em;padding:2em}h1{font:600 1.6em Georgia,serif;margin:0 0 .4em}p{margin:0;opacity:.7}</style></head><body><main><h1>You're signed in</h1><p>Close this tab and go back to Cipher.</p></main></body></html>"#;

/// Reads one HTTP request. Returns the query parameters if it is the callback.
fn handle_request(stream: TcpStream) -> Option<Vec<(String, String)>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let target = line.split_whitespace().nth(1).unwrap_or("").to_string();
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let is_callback = path == CALLBACK_PATH;
    let page = if is_callback {
        CALLBACK_PAGE
    } else {
        "Not found"
    };
    let status = if is_callback {
        "200 OK"
    } else {
        "404 Not Found"
    };
    let mut stream = reader.into_inner();
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    is_callback.then(|| parse_query(query))
}

fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push((h * 16 + l) as u8);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// An error answer from the token endpoint.
#[derive(Debug)]
enum TokenError {
    /// The OAuth error code (`invalid_grant`, `refresh_token_reused`, …).
    OAuth(String),
    Network(String),
    Other(String),
}

impl TokenError {
    fn describe(&self) -> String {
        match self {
            TokenError::OAuth(c) => format!("the token endpoint said {c}"),
            TokenError::Network(e) => format!("network error: {e}"),
            TokenError::Other(e) => e.clone(),
        }
    }
}

fn post_form(url: &str, form: &[(&str, &str)]) -> Result<Value, TokenError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    match agent
        .post(url)
        .set("Accept", "application/json")
        .send_form(form)
    {
        Ok(r) => r.into_json().map_err(|e| TokenError::Other(e.to_string())),
        Err(ureq::Error::Status(code, r)) => {
            let body: Value = r.into_json().unwrap_or(Value::Null);
            let err = body
                .get("error")
                .and_then(|e| {
                    e.as_str()
                        .map(str::to_owned)
                        .or_else(|| e.get("code").and_then(Value::as_str).map(str::to_owned))
                })
                .unwrap_or_else(|| format!("HTTP {code}"));
            Err(TokenError::OAuth(err))
        }
        Err(ureq::Error::Transport(t)) => Err(TokenError::Network(t.to_string())),
    }
}

fn str_field(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_owned)
}

/// Credentials from a token answer. `claims` are the ID token's; `previous` fills in what a
/// refresh answer leaves out.
fn credentials(
    tokens: &Value,
    claims: &Value,
    id_token: String,
    previous: Option<&Credentials>,
) -> Result<Credentials, &'static str> {
    let access_token = str_field(tokens, "access_token").ok_or("no access token")?;
    // Refresh tokens rotate; an answer without one keeps the old one.
    let refresh_token = str_field(tokens, "refresh_token")
        .or_else(|| previous.map(|p| p.refresh_token.clone()))
        .ok_or("no refresh token")?;
    let expires_in = tokens
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(3600);
    let auth = claims.get(AUTH_CLAIM);
    let claim = |k: &str| {
        auth.and_then(|a| a.get(k))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let account = claim("chatgpt_account_id")
        .or_else(|| previous.map(|p| p.account.clone()))
        .ok_or("this sign-in has no ChatGPT account")?;
    Ok(Credentials {
        account,
        email: claims
            .get("email")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| previous.and_then(|p| p.email.clone())),
        plan: claim("chatgpt_plan_type").or_else(|| previous.and_then(|p| p.plan.clone())),
        access_token,
        refresh_token,
        expires_at: now() + expires_in,
        id_token,
    })
}

/// A JWT's claims, unverified. Only for tokens straight from the token endpoint.
fn jwt_claims(token: &str) -> Value {
    token
        .split('.')
        .nth(1)
        .and_then(|p| B64.decode(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null)
}

fn audience(claims: &Value) -> Vec<String> {
    match claims.get("aud") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// Checks an RS256 ID token's signature (against the issuer's JWKS), issuer and expiry,
/// and returns its claims. The caller checks audience and nonce.
fn verify_id_token(issuer: &Issuer, token: &str) -> Result<Value, SignInError> {
    let bad = |why: &str| SignInError::Failed(format!("invalid ID token: {why}"));
    let mut parts = token.split('.');
    let (Some(h), Some(p), Some(sig), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(bad("not a JWT"));
    };
    let json = |s: &str| -> Result<Value, SignInError> {
        let bytes = B64.decode(s).map_err(|_| bad("bad encoding"))?;
        serde_json::from_slice(&bytes).map_err(|_| bad("bad JSON"))
    };
    let header = json(h)?;
    let claims = json(p)?;

    if let Some(jwks_url) = &issuer.jwks {
        if header.get("alg").and_then(Value::as_str) != Some("RS256") {
            return Err(bad("unexpected algorithm"));
        }
        let kid = header
            .get("kid")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("no key id"))?;
        let jwks: Value = ureq::get(jwks_url)
            .timeout(Duration::from_secs(30))
            .call()
            .map_err(|e| SignInError::Failed(format!("could not fetch signing keys: {e}")))?
            .into_json()
            .map_err(|_| bad("bad key set"))?;
        let key = jwks
            .get("keys")
            .and_then(Value::as_array)
            .and_then(|keys| {
                keys.iter()
                    .find(|k| k.get("kid").and_then(Value::as_str) == Some(kid))
            })
            .ok_or_else(|| bad("unknown signing key"))?;
        let component = |name: &str| -> Result<Vec<u8>, SignInError> {
            let s = key
                .get(name)
                .and_then(Value::as_str)
                .ok_or_else(|| bad("bad key"))?;
            B64.decode(s).map_err(|_| bad("bad key"))
        };
        let public = ring::signature::RsaPublicKeyComponents {
            n: component("n")?,
            e: component("e")?,
        };
        let signature = B64.decode(sig).map_err(|_| bad("bad signature encoding"))?;
        public
            .verify(
                &ring::signature::RSA_PKCS1_2048_8192_SHA256,
                format!("{h}.{p}").as_bytes(),
                &signature,
            )
            .map_err(|_| bad("signature does not verify"))?;
    }
    if claims.get("iss").and_then(Value::as_str) != Some(issuer.issuer.as_str()) {
        return Err(bad("wrong issuer"));
    }
    if claims
        .get("exp")
        .and_then(Value::as_i64)
        .is_none_or(|exp| exp + 60 < now())
    {
        return Err(bad("expired"));
    }
    Ok(claims)
}

/// A signed-in ChatGPT account for one Endpoint. Hands out access tokens, refreshing them
/// before they expire and writing the rotated tokens back to [`Secrets`].
pub struct ChatGptSession {
    issuer: Issuer,
    secrets: Arc<dyn Secrets>,
    secret_id: String,
    creds: Mutex<Credentials>,
}

impl ChatGptSession {
    /// The Secrets id where an Endpoint's ChatGPT Credentials live.
    pub fn secret_id(endpoint_id: &str) -> String {
        format!("endpoint/{endpoint_id}/chatgpt")
    }

    /// Stores freshly signed-in Credentials for an Endpoint.
    pub fn save(
        secrets: &dyn Secrets,
        endpoint_id: &str,
        creds: &Credentials,
    ) -> Result<(), LlmError> {
        let json = serde_json::to_string(creds).map_err(|e| LlmError::Rejected(e.to_string()))?;
        secrets
            .set(&Self::secret_id(endpoint_id), &json)
            .map_err(|e| LlmError::Rejected(e.to_string()))
    }

    /// The Credentials stored for an Endpoint, if it has signed in. Credentials from the
    /// earlier Sign in with ChatGPT flow read as not signed in.
    pub fn stored(
        secrets: &dyn Secrets,
        endpoint_id: &str,
    ) -> Result<Option<Credentials>, LlmError> {
        let raw = secrets
            .get(&Self::secret_id(endpoint_id))
            .map_err(|e| LlmError::Rejected(e.to_string()))?;
        // The earlier flow stored its own `client_id`; its tokens don't work here.
        Ok(raw
            .and_then(|r| serde_json::from_str::<Value>(&r).ok())
            .filter(|v| v.get("client_id").is_none())
            .and_then(|v| serde_json::from_value(v).ok()))
    }

    /// Opens the session for an Endpoint. `Auth` when it has never signed in.
    pub fn open(
        issuer: Issuer,
        secrets: Arc<dyn Secrets>,
        endpoint_id: &str,
    ) -> Result<ChatGptSession, LlmError> {
        let creds = Self::stored(secrets.as_ref(), endpoint_id)?.ok_or(LlmError::Auth)?;
        Ok(ChatGptSession {
            issuer,
            secrets,
            secret_id: Self::secret_id(endpoint_id),
            creds: Mutex::new(creds),
        })
    }

    pub fn credentials(&self) -> Credentials {
        self.lock().clone()
    }

    /// The Codex backend this session's requests go to.
    pub fn backend(&self) -> &str {
        &self.issuer.backend
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Credentials> {
        self.creds.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A usable access token, refreshed first if it is about to expire.
    pub fn access_token(&self) -> Result<String, LlmError> {
        let (token, expires_at) = {
            let c = self.lock();
            (c.access_token.clone(), c.expires_at)
        };
        if expires_at - REFRESH_MARGIN > now() {
            return Ok(token);
        }
        self.refresh()
    }

    /// Refreshes now. Refresh tokens rotate: the new set replaces the old one together.
    ///
    /// An unusable refresh token is `Auth` (sign in again). A network or server failure
    /// keeps the stored Credentials.
    pub fn refresh(&self) -> Result<String, LlmError> {
        let mut creds = self.lock();
        let answer = post_form(
            &self.issuer.token,
            &[
                ("grant_type", "refresh_token"),
                ("client_id", &self.issuer.client_id),
                ("refresh_token", &creds.refresh_token),
            ],
        );
        let tokens = match answer {
            Ok(t) => t,
            Err(TokenError::OAuth(code)) => {
                return Err(match code.as_str() {
                    "invalid_grant"
                    | "invalid_refresh_token"
                    | "token_expired"
                    | "refresh_token_expired"
                    | "refresh_token_invalidated"
                    | "refresh_token_reused" => LlmError::Auth,
                    other if other.starts_with("HTTP 5") => LlmError::Server(other.into()),
                    other => LlmError::Rejected(format!(
                        "refreshing the ChatGPT sign-in failed: {other}"
                    )),
                });
            }
            Err(TokenError::Network(e)) => return Err(LlmError::Network(e)),
            Err(TokenError::Other(e)) => return Err(LlmError::Server(e)),
        };
        let id_token = str_field(&tokens, "id_token").unwrap_or_else(|| creds.id_token.clone());
        let next = credentials(&tokens, &jwt_claims(&id_token), id_token, Some(&creds))
            .map_err(|e| LlmError::Server(e.to_string()))?;
        let json = serde_json::to_string(&next).map_err(|e| LlmError::Server(e.to_string()))?;
        self.secrets
            .set(&self.secret_id, &json)
            .map_err(|e| LlmError::Rejected(e.to_string()))?;
        *creds = next;
        Ok(creds.access_token.clone())
    }

    /// The models the plan offers in Codex, best first: `(slug, name)`.
    pub fn models(&self) -> Result<Vec<(String, String)>, LlmError> {
        let token = self.access_token()?;
        let account = self.lock().account.clone();
        let url = format!(
            "{}/models?client_version={CLIENT_VERSION}",
            self.issuer.backend
        );
        let answer = ureq::get(&url)
            .timeout(Duration::from_secs(30))
            .set("Authorization", &format!("Bearer {token}"))
            .set("chatgpt-account-id", &account)
            .set("originator", ORIGINATOR)
            .call();
        let body: Value = match answer {
            Ok(r) => r.into_json().map_err(|e| LlmError::Server(e.to_string()))?,
            Err(ureq::Error::Status(401 | 403, _)) => return Err(LlmError::Auth),
            Err(ureq::Error::Status(code, _)) => {
                return Err(LlmError::Server(format!(
                    "the model list answered HTTP {code}"
                )));
            }
            Err(ureq::Error::Transport(t)) => return Err(LlmError::Network(t.to_string())),
        };
        Ok(parse_models(&body))
    }
}

/// The listed models of a Codex `/models` answer, by priority.
fn parse_models(body: &Value) -> Vec<(String, String)> {
    let mut models: Vec<(i64, String, String)> = body
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| {
            m.get("visibility")
                .and_then(Value::as_str)
                .is_none_or(|v| v == "list")
        })
        .filter_map(|m| {
            let slug = m.get("slug").and_then(Value::as_str)?.to_string();
            let name = m
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or(&slug)
                .to_string();
            let priority = m.get("priority").and_then(Value::as_i64).unwrap_or(0);
            Some((priority, slug, name))
        })
        .collect();
    models.sort_by_key(|m| m.0);
    models.into_iter().map(|(_, s, n)| (s, n)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemory;
    use std::io::Read;

    #[test]
    fn pkce_matches_rfc_7636() {
        assert_eq!(
            code_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn query_round_trip() {
        assert_eq!(encode("a b/c+é"), "a%20b%2Fc%2B%C3%A9");
        assert_eq!(
            parse_query("code=a%2Fb&state=x+y&e"),
            [
                ("code".into(), "a/b".into()),
                ("state".into(), "x y".into()),
                ("e".into(), String::new())
            ]
        );
    }

    /// A stub token endpoint answering each request in turn, recording the request bodies.
    fn stub(answers: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/token", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for (status, body) in answers {
                let (mut s, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(s.try_clone().unwrap());
                let mut len = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut buf = vec![0; len];
                reader.read_exact(&mut buf).unwrap();
                log.lock().unwrap().push(String::from_utf8(buf).unwrap());
                write!(s, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (url, seen)
    }

    fn jwt(claims: Value) -> String {
        format!(
            "{}.{}.sig",
            B64.encode(r#"{"alg":"RS256","kid":"k"}"#),
            B64.encode(claims.to_string())
        )
    }

    fn test_issuer(token: String) -> Issuer {
        Issuer {
            issuer: "https://auth.example".into(),
            authorize: "https://auth.example/oauth/authorize".into(),
            token,
            jwks: None,
            client_id: "app_test".into(),
            backend: "https://backend.example/codex".into(),
            // Any free port, so tests run side by side.
            ports: vec![0],
        }
    }

    #[test]
    fn sign_in_exchanges_the_code_and_reads_the_account() {
        let id = jwt(serde_json::json!({
            "iss": "https://auth.example", "aud": ["app_test"], "sub": "user-1",
            "email": "a@b.c", "exp": now() + 600,
            "https://api.openai.com/auth": {"chatgpt_account_id": "acct-9", "chatgpt_plan_type": "free"}
        }));
        let body = serde_json::json!({"access_token":"at","refresh_token":"rt","id_token":id,"expires_in":3600}).to_string();
        let (token_url, seen) = stub(vec![(200, body)]);
        let sign_in = SignIn::start(test_issuer(token_url), Some("a@b.c")).unwrap();
        let url = sign_in.url().to_string();
        for want in [
            "client_id=app_test",
            "response_type=code",
            "code_challenge_method=S256",
            "scope=openid%20profile%20email%20offline_access",
            "id_token_add_organizations=true",
            "codex_cli_simplified_flow=true",
            "originator=codex_cli_rs",
            "login_hint=a%40b.c",
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A",
        ] {
            assert!(url.contains(want), "{want} missing from {url}");
        }
        let param = |k: &str| {
            parse_query(url.split_once('?').unwrap().1)
                .into_iter()
                .find(|(n, _)| n == k)
                .unwrap()
                .1
        };
        let (state, redirect) = (param("state"), param("redirect_uri"));
        let browser = std::thread::spawn(move || {
            let rest = redirect.trim_start_matches("http://");
            let (addr, path) = rest.split_once('/').unwrap();
            let mut s = TcpStream::connect(addr).unwrap();
            write!(
                s,
                "GET /{path}?code=c0de&state={} HTTP/1.1\r\nHost: x\r\n\r\n",
                encode(&state)
            )
            .unwrap();
            let mut page = String::new();
            s.read_to_string(&mut page).unwrap();
            page
        });
        let creds = sign_in.finish(Duration::from_secs(10)).unwrap();
        assert!(browser.join().unwrap().contains("signed in"));
        assert_eq!(
            (creds.account.as_str(), creds.access_token.as_str()),
            ("acct-9", "at")
        );
        assert_eq!(creds.describe(), "a@b.c · Free plan");
        let form = seen.lock().unwrap()[0].clone();
        for want in [
            "grant_type=authorization_code",
            "client_id=app_test",
            "code=c0de",
            "code_verifier=",
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A",
        ] {
            assert!(form.contains(want), "{want} missing from {form}");
        }
        assert!(!form.contains("resource="), "{form}");
    }

    fn stored_creds(expires_at: i64) -> Credentials {
        Credentials {
            account: "acct".into(),
            email: Some("a@b.c".into()),
            plan: Some("free".into()),
            access_token: "old".into(),
            refresh_token: "r1".into(),
            expires_at,
            id_token: "id".into(),
        }
    }

    #[test]
    fn session_refreshes_before_expiry_and_rotates() {
        let (url, seen) = stub(vec![(
            200,
            r#"{"access_token":"new","refresh_token":"r2","expires_in":3600}"#.into(),
        )]);
        let secrets: Arc<dyn Secrets> = Arc::new(InMemory::new());
        ChatGptSession::save(secrets.as_ref(), "e", &stored_creds(now() + 30)).unwrap();
        let session = ChatGptSession::open(test_issuer(url), secrets.clone(), "e").unwrap();
        assert_eq!(session.access_token().unwrap(), "new");
        let saved = ChatGptSession::stored(secrets.as_ref(), "e")
            .unwrap()
            .unwrap();
        assert_eq!(
            (saved.refresh_token.as_str(), saved.account.as_str()),
            ("r2", "acct")
        );
        let form = seen.lock().unwrap()[0].clone();
        assert!(form.contains("grant_type=refresh_token") && form.contains("client_id=app_test"));
        // Fresh now: no second request.
        assert_eq!(session.access_token().unwrap(), "new");
    }

    #[test]
    fn a_dead_refresh_token_means_sign_in_again_but_keeps_nothing_broken() {
        let (url, _) = stub(vec![
            (400, r#"{"error":"refresh_token_reused"}"#.into()),
            (503, "{}".into()),
        ]);
        let secrets: Arc<dyn Secrets> = Arc::new(InMemory::new());
        ChatGptSession::save(secrets.as_ref(), "e", &stored_creds(0)).unwrap();
        let session = ChatGptSession::open(test_issuer(url), secrets.clone(), "e").unwrap();
        assert_eq!(session.access_token(), Err(LlmError::Auth));
        assert!(matches!(session.refresh(), Err(LlmError::Server(_))));
        assert_eq!(
            ChatGptSession::stored(secrets.as_ref(), "e")
                .unwrap()
                .unwrap()
                .refresh_token,
            "r1"
        );
    }

    #[test]
    fn old_sign_ins_read_as_signed_out() {
        let secrets = InMemory::new();
        secrets
            .set(
                &ChatGptSession::secret_id("e"),
                r#"{"client_id":"oaiapp_1","account":"u","email":null,"access_token":"a","refresh_token":"r","expires_at":0,"scopes":[],"id_token":"i"}"#,
            )
            .unwrap();
        assert_eq!(ChatGptSession::stored(&secrets, "e").unwrap(), None);
    }

    #[test]
    fn models_are_the_listed_ones_by_priority() {
        let body = serde_json::json!({"models": [
            {"slug": "b", "display_name": "B", "priority": 2, "visibility": "list"},
            {"slug": "hidden", "display_name": "H", "priority": 0, "visibility": "hide"},
            {"slug": "a", "display_name": "A", "priority": 1, "visibility": "list"}
        ]});
        assert_eq!(
            parse_models(&body),
            [("a".into(), "A".into()), ("b".into(), "B".into())]
        );
    }

    #[test]
    fn claims_are_checked() {
        let issuer = test_issuer("x".into());
        let ok = jwt(serde_json::json!({"iss":"https://auth.example","exp":now()+60}));
        assert!(verify_id_token(&issuer, &ok).is_ok());
        let wrong_iss = jwt(serde_json::json!({"iss":"https://evil","exp":now()+60}));
        assert!(verify_id_token(&issuer, &wrong_iss).is_err());
        let expired = jwt(serde_json::json!({"iss":"https://auth.example","exp":now()-3600}));
        assert!(verify_id_token(&issuer, &expired).is_err());
    }
}
