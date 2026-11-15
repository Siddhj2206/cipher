use std::time::{Duration, SystemTime};

/// Why a model call failed. The engine decides what to do with each kind:
/// back off, pause until the usage limit resets, ask for a new key, or fail the Job.
///
/// `Network` can arrive mid-stream. When it does, `on_text` has already received
/// the partial text, so the engine keeps its own copy and resumes from it.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LlmError {
    /// Too many requests right now (HTTP 429). Retry after the delay, if the server gave one.
    #[error("rate limited{}", fmt_retry(.retry_after))]
    RateLimited { retry_after: Option<Duration> },
    /// The plan or account quota is spent. A normal pause, not a failure (ADR 0006).
    /// `resets_at` is set when the server said when.
    #[error("usage limit reached")]
    UsageLimit { resets_at: Option<SystemTime> },
    /// The key or token is missing, wrong or expired (HTTP 401 or 403).
    #[error("authentication failed")]
    Auth,
    /// The connection failed, timed out or dropped.
    #[error("network error: {0}")]
    Network(String),
    /// The server failed (HTTP 5xx, or an error event inside the stream).
    #[error("server error: {0}")]
    Server(String),
    /// The server refused the request (other HTTP 4xx), or Cipher cannot make it.
    #[error("request rejected: {0}")]
    Rejected(String),
}

fn fmt_retry(d: &Option<Duration>) -> String {
    match d {
        Some(d) => format!(", retry after {}s", d.as_secs()),
        None => String::new(),
    }
}
