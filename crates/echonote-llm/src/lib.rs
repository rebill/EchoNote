//! LLM features for EchoNote. Requests go out from Rust only; the WebView never sees a key.
//!
//! - [`Client`]: streaming chat against an OpenAI-compatible server (`/chat/completions`,
//!   which covers most hosted services, Ollama and LM Studio) or Anthropic (`/messages`).
//!   Retries rate limits and server errors before any text arrived; never follows redirects.
//! - [`Chat`]: what the features need from a client, so tests can use a fake.
//! - [`summary`] (notes for a transcript of any length), [`qa`], [`correct`] and
//!   [`highlights`] are built on `Chat`; [`testing::FakeChat`] scripts them in tests.
//!
//! Errors and logs never carry the key, extra header values, prompts or transcript text.

mod cancel;
mod client;
pub mod correct;
mod endpoint;
mod error;
pub mod highlights;
pub mod prompts;
pub mod qa;
mod sse;
pub mod summary;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod tokens;
pub mod transcript;

pub use cancel::Cancel;
pub use client::{Chat, Client, Finish, Message, Policy, Request, Role, complete};
pub use endpoint::{Api, Endpoint};
pub use error::LlmError;
