//! Endpoint configuration and `connect`.

use serde::{Deserialize, Serialize};

use std::sync::Arc;

use crate::chat::{ChatCompletions, Timeouts};
use crate::{Auth, ChatGptSession, Effort, Issuer, LlmError, Model, Responses, Secrets};

/// An LLM service the user brings (see CONTEXT.md). Endpoints belong to the
/// user, not the Book. This struct holds no secret: the API key lives in
/// [`Secrets`] under [`Endpoint::api_key_id`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub protocol: Protocol,
    pub base_url: String,
    pub translate_model: String,
    /// A cheaper model for Repair, source resolution and Story Notes compaction.
    #[serde(default)]
    pub small_model: Option<String>,
    #[serde(default)]
    pub reasoning: Effort,
    /// More models a Book may choose on this Endpoint, besides `translate_model`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
}

impl Endpoint {
    /// Every model a Book may choose here: the translate model first, then the others.
    pub fn all_models(&self) -> Vec<String> {
        let mut out = vec![self.translate_model.clone()];
        for m in &self.models {
            if !m.is_empty() && !out.contains(m) {
                out.push(m.clone());
            }
        }
        out
    }

    /// The `Secrets` id of this Endpoint's API key.
    pub fn api_key_id(&self) -> String {
        format!("endpoint/{}/api-key", self.id)
    }
}

/// The wire protocol (ADR 0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    ChatCompletions,
    Responses,
    /// Responses with Sign in with ChatGPT tokens. `base_url` may be empty for the default.
    ChatGpt,
}

/// An Endpoint's two models, ready to call.
pub struct Connection {
    pub translate: Box<dyn Model>,
    /// The cheaper model for Repair, source resolution and compaction; the translate
    /// model again when the Endpoint names none.
    pub small: Box<dyn Model>,
}

/// Builds an Endpoint's models, reading its credentials from `secrets`.
///
/// - Chat Completions and Responses use the API key under [`Endpoint::api_key_id`]. A
///   missing key is not an error here: local servers need none, and a server that does
///   answers 401, which surfaces as [`LlmError::Auth`].
/// - ChatGPT uses the Credentials of a past sign-in ([`ChatGptSession`]); both models share
///   one session, because refresh tokens rotate. Never signed in: [`LlmError::Auth`].
pub fn connect(endpoint: &Endpoint, secrets: Arc<dyn Secrets>) -> Result<Connection, LlmError> {
    let translate_model = endpoint.translate_model.clone();
    let small_model = endpoint
        .small_model
        .clone()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| translate_model.clone());
    let key = || {
        secrets
            .get(&endpoint.api_key_id())
            .map_err(|e| LlmError::Rejected(format!("could not read the API key: {e}")))
    };
    let t = Timeouts::default;
    Ok(match endpoint.protocol {
        Protocol::ChatCompletions => {
            let key = key()?;
            Connection {
                translate: Box::new(ChatCompletions::new(
                    &endpoint.base_url,
                    translate_model,
                    key.clone(),
                    t(),
                )),
                small: Box::new(ChatCompletions::new(
                    &endpoint.base_url,
                    small_model,
                    key,
                    t(),
                )),
            }
        }
        Protocol::Responses => {
            let auth = key()?
                .filter(|k| !k.is_empty())
                .map_or(Auth::None, Auth::ApiKey);
            Connection {
                translate: Box::new(Responses::new(
                    &endpoint.base_url,
                    translate_model,
                    auth.clone(),
                    t(),
                )),
                small: Box::new(Responses::new(&endpoint.base_url, small_model, auth, t())),
            }
        }
        Protocol::ChatGpt => {
            let issuer = Issuer::openai();
            let base = if endpoint.base_url.is_empty() {
                issuer.backend.clone()
            } else {
                endpoint.base_url.clone()
            };
            let session = Arc::new(ChatGptSession::open(issuer, secrets.clone(), &endpoint.id)?);
            Connection {
                translate: Box::new(Responses::new(
                    &base,
                    translate_model,
                    Auth::ChatGpt(session.clone()),
                    t(),
                )),
                small: Box::new(Responses::new(
                    &base,
                    small_model,
                    Auth::ChatGpt(session),
                    t(),
                )),
            }
        }
    })
}

/// The user's Endpoints, kept in a JSON file outside any Book. It holds no secret: keys and
/// tokens are in [`Secrets`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Endpoints {
    pub endpoints: Vec<Endpoint>,
    /// The Endpoint new work uses.
    #[serde(default)]
    pub default: Option<String>,
}

impl Endpoints {
    /// `$XDG_CONFIG_HOME/cipher/endpoints.json`, or `~/.config/cipher/endpoints.json`; on
    /// Windows `%APPDATA%\cipher`, on macOS `~/Library/Application Support/cipher`. The app's
    /// other files (`prefs.json`, `shelf.json`) live beside it.
    pub fn default_path() -> Option<std::path::PathBuf> {
        #[cfg(target_os = "linux")]
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config"))
            })?;
        #[cfg(not(target_os = "linux"))]
        let base = dirs::config_dir()?;
        Some(base.join("cipher").join("endpoints.json"))
    }

    /// A missing file is an empty list.
    pub fn load(path: &std::path::Path) -> std::io::Result<Endpoints> {
        match std::fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Endpoints::default()),
            Err(e) => Err(e),
        }
    }

    /// Writes atomically: a temporary file next to it, then a rename.
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }

    /// The default Endpoint, or the first one.
    pub fn current(&self) -> Option<&Endpoint> {
        self.default
            .as_deref()
            .and_then(|id| self.endpoints.iter().find(|e| e.id == id))
            .or_else(|| self.endpoints.first())
    }

    /// The Endpoint a Book translates with: the one it chose (`endpoint` id), translating with
    /// its chosen `model`, or the default Endpoint when it chose none or its choice is gone.
    pub fn for_book(&self, choice: Option<(&str, &str)>) -> Option<Endpoint> {
        if let Some((id, model)) = choice
            && let Some(e) = self.endpoints.iter().find(|e| e.id == id)
            && !model.is_empty()
        {
            let mut e = e.clone();
            e.translate_model = model.to_string();
            return Some(e);
        }
        self.current().cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(id: &str, model: &str) -> Endpoint {
        Endpoint {
            id: id.into(),
            name: id.into(),
            protocol: Protocol::ChatCompletions,
            base_url: "https://example.com/v1".into(),
            translate_model: model.into(),
            small_model: None,
            reasoning: Effort::ProviderDefault,
            models: vec!["m-extra".into(), model.into()],
        }
    }

    #[test]
    fn a_book_uses_its_chosen_endpoint_and_model_else_the_default() {
        let all = Endpoints {
            endpoints: vec![endpoint("a", "m-a"), endpoint("b", "m-b")],
            default: Some("b".into()),
        };
        assert_eq!(all.endpoints[0].all_models(), ["m-a", "m-extra"]);
        let chosen = all.for_book(Some(("a", "m-extra"))).unwrap();
        assert_eq!(
            (chosen.id.as_str(), chosen.translate_model.as_str()),
            ("a", "m-extra")
        );
        assert_eq!(all.for_book(None).unwrap().id, "b");
        assert_eq!(all.for_book(Some(("gone", "x"))).unwrap().id, "b");
    }

    #[test]
    fn endpoints_file_round_trips_and_defaults() {
        let dir = std::env::temp_dir().join(format!("cipher-endpoints-{}", std::process::id()));
        let path = dir.join("endpoints.json");
        assert_eq!(Endpoints::load(&path).unwrap(), Endpoints::default());
        let ep = |id: &str| Endpoint {
            id: id.into(),
            name: id.into(),
            protocol: Protocol::ChatCompletions,
            base_url: "https://opencode.ai/zen/go/v1".into(),
            translate_model: "m".into(),
            small_model: None,
            reasoning: Effort::Low,
            models: Vec::new(),
        };
        let mut all = Endpoints {
            endpoints: vec![ep("a"), ep("b")],
            default: None,
        };
        assert_eq!(all.current().unwrap().id, "a");
        all.default = Some("b".into());
        all.save(&path).unwrap();
        let back = Endpoints::load(&path).unwrap();
        assert_eq!(back, all);
        assert_eq!(back.current().unwrap().id, "b");
        let _ = std::fs::remove_dir_all(dir);
    }
}
