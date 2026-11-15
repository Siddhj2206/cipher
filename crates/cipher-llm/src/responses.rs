//! The Responses adapter: `POST {base}/responses` with `stream: true`, for providers that
//! serve some models only this way and for ChatGPT plans (ADR 0006).

use std::io::{BufRead, BufReader};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::chat::{Sse, Timeouts, excerpt, map_status};
use crate::chatgpt::ChatGptSession;
use crate::{Completion, LlmError, Model, Request, Usage};

/// How a Responses endpoint authenticates.
#[derive(Clone)]
pub enum Auth {
    /// No `Authorization` header (local servers).
    None,
    ApiKey(String),
    /// Sign in with ChatGPT: a bearer token that refreshes itself.
    ChatGpt(Arc<ChatGptSession>),
}

pub struct Responses {
    url: String,
    model: String,
    auth: Auth,
    agent: ureq::Agent,
}

impl Responses {
    /// `base_url` is the part before `/responses`, e.g. `https://api.openai.com/v1`.
    pub fn new(base_url: &str, model: impl Into<String>, auth: Auth, timeouts: Timeouts) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(timeouts.connect)
            .timeout_read(timeouts.read)
            .timeout_write(timeouts.connect)
            .build();
        Responses {
            url: format!("{}/responses", base_url.trim_end_matches('/')),
            model: model.into(),
            auth,
            agent,
        }
    }

    pub(crate) fn body(&self, req: &Request) -> Value {
        let mut body = json!({
            "model": self.model,
            "instructions": req.instructions,
            "input": req.input,
            "stream": true,
            // ChatGPT plans require it; nobody else needs the response kept either.
            "store": false,
            // Caps visible and reasoning tokens together.
            "max_output_tokens": req.max_output,
        });
        if let Some(effort) = req.reasoning.wire_value() {
            body["reasoning"] = json!({ "effort": effort });
        }
        // The Codex backend refuses an output cap (the plan's limits bound it instead) and takes
        // `input` only as a list of messages.
        if matches!(self.auth, Auth::ChatGpt(_))
            && let Some(o) = body.as_object_mut()
        {
            o.remove("max_output_tokens");
            o.insert(
                "input".into(),
                json!([{
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": req.input }],
                }]),
            );
        }
        body
    }

    fn send(&self, req: &Request, token: Option<&str>) -> Result<ureq::Response, (u16, LlmError)> {
        let mut http = self
            .agent
            .post(&self.url)
            .set("Accept", "text/event-stream");
        if let Some(t) = token {
            http = http.set("Authorization", &format!("Bearer {t}"));
        }
        if let Auth::ChatGpt(session) = &self.auth {
            http = http
                .set("chatgpt-account-id", &session.credentials().account)
                .set("OpenAI-Beta", "responses=experimental")
                .set("originator", crate::chatgpt::ORIGINATOR);
        }
        match http.send_json(self.body(req)) {
            Ok(r) => Ok(r),
            Err(ureq::Error::Status(code, resp)) => {
                let retry_after = resp.header("Retry-After").map(str::to_owned);
                let body = resp.into_string().unwrap_or_default();
                Err((code, map_error(code, retry_after.as_deref(), &body)))
            }
            Err(ureq::Error::Transport(t)) => Err((0, LlmError::Network(t.to_string()))),
        }
    }
}

impl Model for Responses {
    fn complete(
        &self,
        req: &Request,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError> {
        let response = match &self.auth {
            Auth::None => self.send(req, None),
            Auth::ApiKey(key) => self.send(req, Some(key)),
            Auth::ChatGpt(session) => {
                let token = session.access_token()?;
                match self.send(req, Some(&token)) {
                    // The token may have been revoked or rotated elsewhere: refresh once.
                    Err((401, _)) => {
                        let token = session.refresh()?;
                        self.send(req, Some(&token))
                    }
                    other => other,
                }
            }
        };
        let response = response.map_err(|(_, e)| e)?;
        read_stream(BufReader::new(response.into_reader()), on_text)
    }
}

/// The Codex backend's usage errors (`openai/codex` `api_bridge.rs`), OpenAI's error codes,
/// then the usual statuses.
pub(crate) fn map_error(status: u16, retry_after: Option<&str>, body: &str) -> LlmError {
    let json = serde_json::from_str::<Value>(body).ok();
    let field = |k: &str| {
        json.as_ref()
            .and_then(|v| v.pointer(&format!("/error/{k}")))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    match field("type").as_deref() {
        Some("usage_limit_reached") => {
            let resets_at = json
                .as_ref()
                .and_then(|v| v.pointer("/error/resets_at"))
                .and_then(Value::as_u64)
                .map(|s| std::time::UNIX_EPOCH + std::time::Duration::from_secs(s));
            return LlmError::UsageLimit { resets_at };
        }
        Some("usage_not_included") => {
            return LlmError::Rejected(
                "this ChatGPT plan doesn't include Codex; sign in with another account".into(),
            );
        }
        _ => {}
    }
    match field("code").as_deref().and_then(map_code) {
        Some(e) => e,
        None => map_status(status, retry_after, body),
    }
}

fn map_code(code: &str) -> Option<LlmError> {
    Some(match code {
        // The plan's share is spent. No reset time can be inferred from the code alone.
        "subscription_sharing_usage_limit_exceeded" | "insufficient_quota" => {
            LlmError::UsageLimit { resets_at: None }
        }
        "subscription_sharing_usage_unavailable" => {
            LlmError::Server("ChatGPT plan usage is unavailable right now".into())
        }
        "subscription_sharing_user_not_eligible" => LlmError::Rejected(
            "this ChatGPT account is not eligible for plan usage in other apps".into(),
        ),
        "subscription_sharing_unsupported_capability" => {
            LlmError::Rejected("the ChatGPT plan does not support this request".into())
        }
        _ => return None,
    })
}

fn read_stream<R: BufRead>(
    reader: R,
    on_text: &mut dyn FnMut(&str),
) -> Result<Completion, LlmError> {
    let mut sse = Sse::new(reader);
    let mut text = String::new();
    let mut usage = Usage::default();
    let mut finished: Option<bool> = None;
    loop {
        let event = match sse.next_event() {
            Ok(Some(e)) => e,
            Ok(None) => break,
            Err(e) => return Err(LlmError::Network(format!("stream interrupted: {e}"))),
        };
        let Ok(event) = serde_json::from_str::<Value>(&event) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "response.output_text.delta" => {
                if let Some(piece) = event.get("delta").and_then(Value::as_str)
                    && !piece.is_empty()
                {
                    text.push_str(piece);
                    on_text(piece);
                }
            }
            "response.completed" => {
                usage = parse_usage(event.pointer("/response/usage"));
                finished = Some(true);
                break;
            }
            // Cut off, usually by `max_output_tokens`.
            "response.incomplete" => {
                usage = parse_usage(event.pointer("/response/usage"));
                finished = Some(false);
                break;
            }
            "response.failed" => {
                let err = event.pointer("/response/error");
                return Err(stream_error(err));
            }
            "error" => return Err(stream_error(Some(&event))),
            _ => {}
        }
    }
    Ok(Completion {
        text,
        usage,
        finished: finished.unwrap_or(false),
    })
}

fn stream_error(err: Option<&Value>) -> LlmError {
    let code = err
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(e) = map_code(code) {
        return e;
    }
    let msg = err
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .map_or_else(
            || err.map(Value::to_string).unwrap_or_default(),
            str::to_owned,
        );
    match code {
        "rate_limit_exceeded" => LlmError::RateLimited { retry_after: None },
        "invalid_prompt" | "invalid_request_error" => LlmError::Rejected(excerpt(&msg)),
        _ => LlmError::Server(format!("error in stream: {}", excerpt(&msg))),
    }
}

fn parse_usage(u: Option<&Value>) -> Usage {
    let Some(u) = u else { return Usage::default() };
    let get = |ptr: &str| u.pointer(ptr).and_then(Value::as_u64);
    Usage {
        input: get("/input_tokens").unwrap_or(0),
        output: get("/output_tokens").unwrap_or(0),
        reasoning: get("/output_tokens_details/reasoning_tokens"),
        cached: get("/input_tokens_details/cached_tokens"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(events: &[&str]) -> String {
        events
            .iter()
            .map(|e| format!("event: x\ndata: {e}\n\n"))
            .collect()
    }

    #[test]
    fn completed_stream_with_usage() {
        let body = stream(&[
            r#"{"type":"response.created","response":{}}"#,
            r#"{"type":"response.output_text.delta","delta":"[1] Hel"}"#,
            r#"{"type":"response.output_text.delta","delta":"lo"}"#,
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":10,"output_tokens":5,"input_tokens_details":{"cached_tokens":4},"output_tokens_details":{"reasoning_tokens":2}}}}"#,
        ]);
        let mut seen = String::new();
        let c = read_stream(body.as_bytes(), &mut |p| seen.push_str(p)).unwrap();
        assert_eq!(
            (c.text.as_str(), seen.as_str(), c.finished),
            ("[1] Hello", "[1] Hello", true)
        );
        assert_eq!(
            c.usage,
            Usage {
                input: 10,
                output: 5,
                reasoning: Some(2),
                cached: Some(4)
            }
        );
    }

    #[test]
    fn incomplete_and_truncated_streams_are_not_finished() {
        let body = stream(&[
            r#"{"type":"response.output_text.delta","delta":"[1] a"}"#,
            r#"{"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}"#,
        ]);
        assert!(!read_stream(body.as_bytes(), &mut |_| {}).unwrap().finished);
        let body = stream(&[r#"{"type":"response.output_text.delta","delta":"[1] a"}"#]);
        let c = read_stream(body.as_bytes(), &mut |_| {}).unwrap();
        assert_eq!((c.text.as_str(), c.finished), ("[1] a", false));
    }

    #[test]
    fn failures_map_to_what_the_engine_acts_on() {
        let failed = |code: &str| {
            let e = format!(
                r#"{{"type":"response.failed","response":{{"error":{{"code":"{code}","message":"m"}}}}}}"#
            );
            read_stream(stream(&[&e]).as_bytes(), &mut |_| {}).unwrap_err()
        };
        assert_eq!(
            failed("subscription_sharing_usage_limit_exceeded"),
            LlmError::UsageLimit { resets_at: None }
        );
        assert!(matches!(
            failed("subscription_sharing_user_not_eligible"),
            LlmError::Rejected(_)
        ));
        assert!(matches!(failed("server_error"), LlmError::Server(_)));

        let body =
            r#"{"error":{"code":"subscription_sharing_usage_limit_exceeded","message":"x"}}"#;
        assert_eq!(
            map_error(429, None, body),
            LlmError::UsageLimit { resets_at: None }
        );
        assert!(matches!(
            map_error(
                503,
                None,
                r#"{"error":{"code":"subscription_sharing_usage_unavailable"}}"#
            ),
            LlmError::Server(_)
        ));
        assert_eq!(map_error(401, None, "{}"), LlmError::Auth);
    }

    #[test]
    fn body_fields() {
        let r = Responses::new(
            "https://api.openai.com/v1/",
            "m",
            Auth::None,
            Timeouts::default(),
        );
        assert_eq!(r.url, "https://api.openai.com/v1/responses");
        let mut req = Request {
            instructions: "i".into(),
            input: "x".into(),
            max_output: 100,
            reasoning: crate::Effort::ProviderDefault,
        };
        let b = r.body(&req);
        assert_eq!(
            (b["store"].clone(), b["stream"].clone()),
            (json!(false), json!(true))
        );
        assert!(b.get("reasoning").is_none());
        req.reasoning = crate::Effort::Low;
        assert_eq!(r.body(&req)["reasoning"]["effort"], "low");
    }

    #[test]
    fn the_codex_backend_gets_input_as_messages_and_no_cap() {
        let secrets: Arc<dyn crate::Secrets> = Arc::new(crate::InMemory::new());
        let creds = crate::Credentials {
            account: "acct".into(),
            email: None,
            plan: None,
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: i64::MAX,
            id_token: "i".into(),
        };
        ChatGptSession::save(secrets.as_ref(), "e", &creds).unwrap();
        let session = ChatGptSession::open(crate::Issuer::openai(), secrets, "e").unwrap();
        let r = Responses::new(
            "https://chatgpt.com/backend-api/codex",
            "m",
            Auth::ChatGpt(Arc::new(session)),
            Timeouts::default(),
        );
        let b = r.body(&Request {
            instructions: "i".into(),
            input: "x".into(),
            max_output: 100,
            reasoning: crate::Effort::ProviderDefault,
        });
        assert!(b.get("max_output_tokens").is_none());
        assert_eq!(b["input"][0]["role"], "user");
        assert_eq!(b["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(b["input"][0]["content"][0]["text"], "x");
        assert_eq!(b["instructions"], "i");
    }

    #[test]
    fn codex_usage_errors() {
        let limit = map_error(
            429,
            None,
            r#"{"error":{"type":"usage_limit_reached","plan_type":"free","resets_at":1900000000}}"#,
        );
        assert_eq!(
            limit,
            LlmError::UsageLimit {
                resets_at: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1900000000))
            }
        );
        assert!(matches!(
            map_error(429, None, r#"{"error":{"type":"usage_not_included"}}"#),
            LlmError::Rejected(_)
        ));
    }
}
