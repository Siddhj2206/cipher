//! The Chat Completions adapter: `POST {base}/chat/completions` with `stream: true`.

use std::io::{BufRead, BufReader};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use crate::{Completion, LlmError, Model, Request, Usage};

/// Timeouts for one HTTP call. There is deliberately no overall limit: a long
/// Chapter can stream for minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// Opening the connection.
    pub connect: Duration,
    /// Waiting for the next bytes. Reasoning models can be silent for a long
    /// time before the first token, so this is generous.
    pub read: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            connect: Duration::from_secs(15),
            read: Duration::from_secs(300),
        }
    }
}

/// An OpenAI-compatible Chat Completions endpoint (OpenRouter, DeepSeek, local servers, ...).
pub struct ChatCompletions {
    url: String,
    model: String,
    api_key: Option<String>,
    agent: ureq::Agent,
}

impl ChatCompletions {
    /// `base_url` is the part before `/chat/completions`, for example
    /// `https://openrouter.ai/api/v1`. A trailing slash is fine. With no
    /// `api_key` no `Authorization` header is sent, which is what local servers expect.
    pub fn new(
        base_url: &str,
        model: impl Into<String>,
        api_key: Option<String>,
        timeouts: Timeouts,
    ) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(timeouts.connect)
            .timeout_read(timeouts.read)
            .timeout_write(timeouts.connect)
            .build();
        ChatCompletions {
            url: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            model: model.into(),
            api_key: api_key.filter(|k| !k.is_empty()),
            agent,
        }
    }

    pub(crate) fn body(&self, req: &Request) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": req.instructions },
                { "role": "user", "content": req.input },
            ],
            "stream": true,
            "stream_options": { "include_usage": true },
            // `max_completion_tokens`, not `max_tokens`: OpenAI deprecated
            // `max_tokens` and its reasoning models reject it, while
            // `max_completion_tokens` also caps hidden reasoning tokens, which
            // is the cap we want. OpenRouter and DeepSeek accept it too.
            "max_completion_tokens": req.max_output,
        });
        if let Some(effort) = req.reasoning.wire_value() {
            body["reasoning_effort"] = json!(effort);
        }
        body
    }
}

impl Model for ChatCompletions {
    fn complete(
        &self,
        req: &Request,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError> {
        let mut http = self
            .agent
            .post(&self.url)
            .set("Accept", "text/event-stream");
        if let Some(key) = &self.api_key {
            http = http.set("Authorization", &format!("Bearer {key}"));
        }
        let response = match http.send_json(self.body(req)) {
            Ok(r) => r,
            Err(ureq::Error::Status(code, resp)) => {
                let retry_after = resp.header("Retry-After").map(str::to_owned);
                let body = resp.into_string().unwrap_or_default();
                return Err(map_status(code, retry_after.as_deref(), &body));
            }
            Err(ureq::Error::Transport(t)) => return Err(LlmError::Network(t.to_string())),
        };
        read_stream(BufReader::new(response.into_reader()), on_text)
    }
}

/// Maps an HTTP error status to the error the engine acts on.
pub(crate) fn map_status(code: u16, retry_after: Option<&str>, body: &str) -> LlmError {
    match code {
        401 | 403 => LlmError::Auth,
        429 => {
            let lower = body.to_lowercase();
            let quota = ["insufficient_quota", "usage_limit", "usage limit"]
                .iter()
                .any(|marker| lower.contains(marker));
            let retry_after_secs = retry_after.and_then(parse_retry_after);
            if quota {
                LlmError::UsageLimit {
                    resets_at: resets_at_from_body(body)
                        .or_else(|| retry_after_secs.map(|d| SystemTime::now() + d)),
                }
            } else {
                LlmError::RateLimited {
                    retry_after: retry_after_secs,
                }
            }
        }
        500..=599 => LlmError::Server(format!("HTTP {code}: {}", excerpt(body))),
        _ => LlmError::Rejected(format!("HTTP {code}: {}", excerpt(body))),
    }
}

/// `Retry-After` as delay seconds. The HTTP-date form is not parsed; callers
/// fall back to their own backoff.
fn parse_retry_after(value: &str) -> Option<Duration> {
    let secs: f64 = value.trim().parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs))
}

/// Some providers say when a plan limit resets: `error.resets_at` (Unix
/// seconds) or `error.resets_in_seconds`.
fn resets_at_from_body(body: &str) -> Option<SystemTime> {
    let v: Value = serde_json::from_str(body).ok()?;
    let err = v.get("error")?;
    if let Some(at) = err.get("resets_at").and_then(Value::as_u64) {
        return Some(SystemTime::UNIX_EPOCH + Duration::from_secs(at));
    }
    let secs = err.get("resets_in_seconds").and_then(Value::as_u64)?;
    Some(SystemTime::now() + Duration::from_secs(secs))
}

pub(crate) fn excerpt(body: &str) -> String {
    const MAX: usize = 500;
    let body = body.trim();
    match body.char_indices().nth(MAX) {
        Some((i, _)) => format!("{}…", &body[..i]),
        None => body.to_owned(),
    }
}

/// Reads server-sent events: `data:` lines joined by newlines, dispatched on a
/// blank line; `:` lines are keep-alive comments. Other fields are ignored.
pub(crate) struct Sse<R> {
    reader: R,
    line: Vec<u8>,
}

impl<R: BufRead> Sse<R> {
    pub(crate) fn new(reader: R) -> Self {
        Sse {
            reader,
            line: Vec::new(),
        }
    }

    /// The data of the next event, or `None` at end of stream. A trailing
    /// event with no closing blank line is still delivered.
    pub(crate) fn next_event(&mut self) -> std::io::Result<Option<String>> {
        let mut data: Option<String> = None;
        loop {
            self.line.clear();
            if self.reader.read_until(b'\n', &mut self.line)? == 0 {
                return Ok(data);
            }
            let line = String::from_utf8_lossy(&self.line);
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if data.is_some() {
                    return Ok(data);
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            if field == "data" {
                let value = value.strip_prefix(' ').unwrap_or(value);
                match &mut data {
                    Some(d) => {
                        d.push('\n');
                        d.push_str(value);
                    }
                    None => data = Some(value.to_owned()),
                }
            }
        }
    }
}

fn read_stream<R: BufRead>(
    reader: R,
    on_text: &mut dyn FnMut(&str),
) -> Result<Completion, LlmError> {
    let mut sse = Sse::new(reader);
    let mut text = String::new();
    let mut usage = Usage::default();
    let mut finish_reason: Option<String> = None;
    let mut done = false;

    loop {
        let event = match sse.next_event() {
            Ok(Some(e)) => e,
            Ok(None) => break,
            Err(e) => return Err(LlmError::Network(format!("stream interrupted: {e}"))),
        };
        if event.trim() == "[DONE]" {
            done = true;
            break;
        }
        // A chunk we cannot parse carries nothing we can use; skip it.
        let Ok(chunk) = serde_json::from_str::<Value>(&event) else {
            continue;
        };
        if let Some(err) = chunk.get("error").filter(|e| !e.is_null()) {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .map_or_else(|| err.to_string(), str::to_owned);
            return Err(LlmError::Server(format!(
                "error in stream: {}",
                excerpt(&msg)
            )));
        }
        if let Some(u) = chunk.get("usage").filter(|u| u.is_object()) {
            usage = parse_usage(u);
        }
        if let Some(choice) = chunk.pointer("/choices/0") {
            if let Some(piece) = choice.pointer("/delta/content").and_then(Value::as_str)
                && !piece.is_empty()
            {
                text.push_str(piece);
                on_text(piece);
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                finish_reason = Some(reason.to_owned());
            }
        }
    }

    let finished = match finish_reason.as_deref() {
        Some("length") => false,
        Some(_) => true,
        None => done,
    };
    Ok(Completion {
        text,
        usage,
        finished,
    })
}

fn parse_usage(u: &Value) -> Usage {
    let get = |ptr: &str| u.pointer(ptr).and_then(Value::as_u64);
    Usage {
        input: get("/prompt_tokens").unwrap_or(0),
        output: get("/completion_tokens").unwrap_or(0),
        reasoning: get("/completion_tokens_details/reasoning_tokens"),
        // DeepSeek reports cache hits under its own name.
        cached: get("/prompt_tokens_details/cached_tokens")
            .or_else(|| get("/prompt_cache_hit_tokens")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(input: &str) -> Vec<String> {
        let mut sse = Sse::new(input.as_bytes());
        let mut out = Vec::new();
        while let Some(e) = sse.next_event().unwrap() {
            out.push(e);
        }
        out
    }

    #[test]
    fn sse_handles_comments_crlf_multiline_and_missing_final_blank_line() {
        let input = ": keep-alive\r\n\r\ndata: one\r\n\r\nevent: x\ndata: a\ndata: b\n\n: ping\n\ndata:tight\n";
        assert_eq!(events(input), ["one", "a\nb", "tight"]);
    }

    #[test]
    fn status_mapping() {
        assert_eq!(map_status(401, None, ""), LlmError::Auth);
        assert_eq!(map_status(403, None, ""), LlmError::Auth);
        assert_eq!(
            map_status(429, Some("7"), "{}"),
            LlmError::RateLimited {
                retry_after: Some(Duration::from_secs(7))
            }
        );
        assert!(matches!(
            map_status(429, None, r#"{"error":{"code":"insufficient_quota"}}"#),
            LlmError::UsageLimit { resets_at: None }
        ));
        assert!(matches!(map_status(503, None, "down"), LlmError::Server(m) if m.contains("down")));
        assert!(matches!(map_status(400, None, "bad"), LlmError::Rejected(m) if m.contains("bad")));
    }

    #[test]
    fn usage_limit_reads_reset_time() {
        let body = r#"{"error":{"type":"usage_limit_reached","resets_at":1800000000}}"#;
        assert_eq!(
            map_status(429, None, body),
            LlmError::UsageLimit {
                resets_at: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000))
            }
        );
    }

    #[test]
    fn excerpt_truncates_on_char_boundary() {
        let long = "한".repeat(600);
        assert_eq!(excerpt(&long).chars().count(), 501);
    }

    #[test]
    fn body_omits_reasoning_effort_by_default() {
        use crate::Effort;
        let m = ChatCompletions::new("http://x/v1/", "m", None, Timeouts::default());
        assert_eq!(m.url, "http://x/v1/chat/completions");
        let mut req = Request {
            instructions: "i".into(),
            input: "u".into(),
            max_output: 10,
            reasoning: Effort::ProviderDefault,
        };
        assert!(m.body(&req).get("reasoning_effort").is_none());
        req.reasoning = Effort::Low;
        assert_eq!(m.body(&req)["reasoning_effort"], "low");
    }
}
