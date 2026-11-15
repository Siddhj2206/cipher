use serde::{Deserialize, Serialize};

use crate::LlmError;

/// One call to a model. Prompts are built by the engine; this crate only carries them.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The system prompt.
    pub instructions: String,
    /// The user message.
    pub input: String,
    /// Upper bound on generated tokens. For reasoning models this includes the
    /// hidden reasoning tokens.
    pub max_output: u32,
    pub reasoning: Effort,
}

/// How hard a reasoning model thinks. Thinking tokens are billed as output and
/// are the hidden cost of a call, so Endpoints default to the provider's choice
/// and let the user turn it down.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    /// Send nothing and let the provider pick.
    #[default]
    ProviderDefault,
    /// The lowest setting, sent as `minimal`.
    #[serde(alias = "off")]
    Minimal,
    Low,
    Medium,
    High,
}

impl Effort {
    /// The `reasoning_effort` wire value, or `None` for the provider default.
    pub fn wire_value(self) -> Option<&'static str> {
        match self {
            Effort::ProviderDefault => None,
            Effort::Minimal => Some("minimal"),
            Effort::Low => Some("low"),
            Effort::Medium => Some("medium"),
            Effort::High => Some("high"),
        }
    }
}

/// The whole response of one call.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub text: String,
    pub usage: Usage,
    /// `false` means the response was cut off: the model hit `max_output`
    /// (finish reason `length`), or the stream ended without a finish reason or
    /// `[DONE]`. `text` is then a usable prefix.
    pub finished: bool,
}

/// Token counts as the provider reports them. All zero when it reports none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    /// Generated tokens. OpenAI-style providers include reasoning tokens here.
    pub output: u64,
    /// The part of `output` spent on hidden reasoning, when the provider says.
    pub reasoning: Option<u64>,
    /// The part of `input` served from the prompt cache, when the provider says.
    pub cached: Option<u64>,
}

impl Usage {
    /// Adds another call's usage to this one, for per-Chapter totals.
    pub fn add(&mut self, other: &Usage) {
        self.input += other.input;
        self.output += other.output;
        self.reasoning = sum_opt(self.reasoning, other.reasoning);
        self.cached = sum_opt(self.cached, other.cached);
    }
}

fn sum_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
    }
}

/// The seam between the engine and any LLM service.
pub trait Model: Send {
    /// Streams text to `on_text` as it arrives and returns the whole response.
    ///
    /// If the call fails after text has started to flow, `on_text` has already
    /// received that partial text. The engine saves partials from `on_text`
    /// and resumes from them, so an `Err` does not mean nothing arrived.
    fn complete(
        &self,
        req: &Request,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError>;
}
