//! Asking an API-key Endpoint which models it serves: `GET {base}/models`, which Chat
//! Completions and Responses servers answer alike (`{"data": [{"id": …}]}`).

use std::time::Duration;

use serde_json::Value;

use crate::chat::{excerpt, map_status};
use crate::{LlmError, Protocol};

/// The model ids `base_url` lists, sorted and without repeats. `api_key` goes in the
/// `Authorization` header only; it never appears in the error, even if the server echoes it.
///
/// ChatGPT Endpoints list their plan's models through [`crate::ChatGptSession::models`].
pub fn list_models(
    protocol: Protocol,
    base_url: &str,
    api_key: Option<&str>,
    timeout: Duration,
) -> Result<Vec<String>, LlmError> {
    if protocol == Protocol::ChatGpt {
        return Err(LlmError::Rejected(
            "a ChatGPT Endpoint lists its plan's models".into(),
        ));
    }
    let key = api_key.map(str::trim).filter(|k| !k.is_empty());
    let url = format!("{}/models", base_url.trim().trim_end_matches('/'));
    let mut http = ureq::get(&url)
        .timeout(timeout)
        .set("Accept", "application/json");
    if let Some(k) = key {
        http = http.set("Authorization", &format!("Bearer {k}"));
    }
    let redact = |e: LlmError| match key {
        Some(k) => scrub(e, k),
        None => e,
    };
    let body = match http.call() {
        Ok(r) => r
            .into_string()
            .map_err(|e| LlmError::Network(e.to_string()))?,
        Err(ureq::Error::Status(code, resp)) => {
            let retry_after = resp.header("Retry-After").map(str::to_owned);
            let body = resp.into_string().unwrap_or_default();
            return Err(redact(map_status(code, retry_after.as_deref(), &body)));
        }
        Err(ureq::Error::Transport(t)) => return Err(redact(LlmError::Network(t.to_string()))),
    };
    let json: Value = serde_json::from_str(&body).map_err(|_| {
        redact(LlmError::Rejected(format!(
            "the model list is not JSON: {}",
            excerpt(&body)
        )))
    })?;
    Ok(parse(&json))
}

/// `data[].id` (OpenAI and most servers), else `models[].id` or `models[].name` (some local
/// servers), else a bare array of objects or strings.
fn parse(body: &Value) -> Vec<String> {
    let items = body
        .get("data")
        .or_else(|| body.get("models"))
        .unwrap_or(body)
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut ids: Vec<String> = items
        .iter()
        .filter_map(|m| {
            m.as_str()
                .or_else(|| m.get("id").and_then(Value::as_str))
                .or_else(|| m.get("name").and_then(Value::as_str))
        })
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(String::from)
        .collect();
    ids.sort_by_key(|id| id.to_lowercase());
    ids.dedup();
    ids
}

fn scrub(e: LlmError, key: &str) -> LlmError {
    let s = |m: String| m.replace(key, "•••");
    match e {
        LlmError::Network(m) => LlmError::Network(s(m)),
        LlmError::Server(m) => LlmError::Server(s(m)),
        LlmError::Rejected(m) => LlmError::Rejected(s(m)),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    const KEY: &str = "sk-test-secret-123";

    /// A stub server answering one request, recording its request line and headers.
    fn stub(status: u16, body: &str) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let body = body.to_string();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(s.try_clone().unwrap());
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                log.lock().unwrap().push(line.trim_end().to_string());
            }
            write!(s, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        (base, seen)
    }

    const T: Duration = Duration::from_secs(5);

    #[test]
    fn both_protocols_get_models_with_the_key_as_a_bearer_token() {
        for protocol in [Protocol::ChatCompletions, Protocol::Responses] {
            let (base, seen) = stub(
                200,
                r#"{"object":"list","data":[{"id":"zeta"},{"id":"Alpha"},{"id":"zeta"},{"id":""}]}"#,
            );
            let models = list_models(protocol, &format!("{base}/"), Some(KEY), T).unwrap();
            assert_eq!(models, ["Alpha", "zeta"]);
            let seen = seen.lock().unwrap();
            assert_eq!(seen[0], "GET /v1/models HTTP/1.1");
            assert!(
                seen.iter()
                    .any(|h| h == &format!("Authorization: Bearer {KEY}"))
            );
        }
    }

    #[test]
    fn no_key_sends_no_authorization_header() {
        let (base, seen) = stub(200, r#"{"models":[{"name":"llama3"}]}"#);
        let models = list_models(Protocol::ChatCompletions, &base, Some("  "), T).unwrap();
        assert_eq!(models, ["llama3"]);
        assert!(
            !seen
                .lock()
                .unwrap()
                .iter()
                .any(|h| h.to_lowercase().starts_with("authorization"))
        );
    }

    #[test]
    fn failures_map_to_errors_that_never_hold_the_key() {
        let (base, _) = stub(401, "{}");
        assert_eq!(
            list_models(Protocol::Responses, &base, Some(KEY), T),
            Err(LlmError::Auth)
        );
        let (base, _) = stub(404, &format!(r#"{{"error":"no route for key {KEY}"}}"#));
        let e = list_models(Protocol::ChatCompletions, &base, Some(KEY), T).unwrap_err();
        assert!(matches!(&e, LlmError::Rejected(m) if m.contains("HTTP 404")));
        assert!(!e.to_string().contains(KEY), "{e}");
        let (base, _) = stub(200, &format!("<html>{KEY}</html>"));
        let e = list_models(Protocol::ChatCompletions, &base, Some(KEY), T).unwrap_err();
        assert!(!format!("{e} {e:?}").contains(KEY), "{e}");
    }

    #[test]
    fn an_unreachable_server_is_a_network_error() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let base = format!("http://127.0.0.1:{port}/v1");
        assert!(matches!(
            list_models(Protocol::ChatCompletions, &base, Some(KEY), T),
            Err(LlmError::Network(_))
        ));
        assert!(list_models(Protocol::ChatGpt, &base, None, T).is_err());
    }
}
