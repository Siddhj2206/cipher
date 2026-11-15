//! A scripted `Model` for engine tests.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::{Completion, LlmError, Model, Request, Usage};

/// One scripted reply. Replies are consumed in order, one per `complete` call.
#[derive(Debug, Clone, PartialEq)]
pub enum FakeReply {
    /// A complete response.
    Text(String),
    /// A response cut off after this text: `finished` is `false`.
    CutOff(String),
    /// The call fails.
    Fail(LlmError),
}

/// Text arrives at `on_text` in pieces this many characters long.
const CHUNK_CHARS: usize = 8;

#[derive(Default)]
struct State {
    script: VecDeque<FakeReply>,
    requests: Vec<Request>,
}

/// Cloning shares the script and the request log, so a test keeps one clone
/// and hands another to the engine as a `Box<dyn Model>`:
///
/// ```
/// use cipher_llm::{Fake, FakeReply};
/// let fake = Fake::new([FakeReply::Text("[1] Hello".into())]);
/// let for_engine = fake.clone();
/// // ... run the engine with Box::new(for_engine) ...
/// assert!(fake.requests().is_empty());
/// ```
///
/// Malformed marker output is just a `Text` with malformed content.
#[derive(Clone, Default)]
pub struct Fake {
    state: Arc<Mutex<State>>,
}

impl Fake {
    pub fn new(script: impl IntoIterator<Item = FakeReply>) -> Self {
        let fake = Fake::default();
        fake.state().script.extend(script);
        fake
    }

    /// Appends a reply to the script.
    pub fn push(&self, reply: FakeReply) {
        self.state().script.push_back(reply);
    }

    /// Every Request received so far, oldest first.
    pub fn requests(&self) -> Vec<Request> {
        self.state().requests.clone()
    }

    /// How many scripted replies have not been used.
    pub fn remaining(&self) -> usize {
        self.state().script.len()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Model for Fake {
    fn complete(
        &self,
        req: &Request,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<Completion, LlmError> {
        let reply = {
            let mut state = self.state();
            state.requests.push(req.clone());
            state.script.pop_front()
        };
        let Some(reply) = reply else {
            let n = self.state().requests.len();
            return Err(LlmError::Rejected(format!(
                "Fake model script ran out: request {n} had no scripted reply"
            )));
        };
        let (text, finished) = match reply {
            FakeReply::Fail(e) => return Err(e),
            FakeReply::Text(t) => (t, true),
            FakeReply::CutOff(t) => (t, false),
        };
        let chars: Vec<char> = text.chars().collect();
        for piece in chars.chunks(CHUNK_CHARS) {
            on_text(&piece.iter().collect::<String>());
        }
        // Rough but deterministic: four characters to a token.
        let tokens = |s: &str| s.chars().count().div_ceil(4) as u64;
        Ok(Completion {
            usage: Usage {
                input: tokens(&req.instructions) + tokens(&req.input),
                output: tokens(&text),
                ..Usage::default()
            },
            text,
            finished,
        })
    }
}
