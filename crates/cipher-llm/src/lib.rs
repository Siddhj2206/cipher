//! The Model seam: one trait for any LLM service, adapters behind it, and the
//! credentials they need. See docs/design/architecture.md › cipher-llm.
//!
//! This crate carries prompts but never builds them; that is the engine's job.

mod chat;
mod chatgpt;
mod endpoint;
mod error;
mod fake;
mod list;
mod model;
mod responses;
mod secrets;

pub use chat::{ChatCompletions, Timeouts};
pub use chatgpt::{ChatGptSession, Credentials, Issuer, SignIn, SignInError};
pub use endpoint::{Connection, Endpoint, Endpoints, Protocol, connect};
pub use error::LlmError;
pub use fake::{Fake, FakeReply};
pub use list::list_models;
pub use model::{Completion, Effort, Model, Request, Usage};
pub use responses::{Auth, Responses};
pub use secrets::{InMemory, Keychain, Secrets, SecretsError};
