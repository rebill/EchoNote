//! LLM features for EchoNote. Requests go out from Rust only; the WebView never sees a key.
//!
//! - [`Client`]: streaming chat against an OpenAI-compatible server (`/chat/completions`,
//!   which covers most hosted services, Ollama and LM Studio) or Anthropic (`/messages`).
//!   Retries rate limits and server errors before any text arrived; never follows redirects.
//! - [`Chat`]: what the features need from a client, so tests can use a fake.
//! - Summaries, Q&A and transcript correction are built on `Chat` in their own modules.
//!
//! Errors and logs never carry the key, extra header values, prompts or transcript text.

mod cancel;
mod client;
mod endpoint;
mod error;
mod sse;
pub mod tokens;

pub use cancel::Cancel;
pub use client::{Chat, Client, Finish, Message, Policy, Request, Role, complete};
pub use endpoint::{Api, Endpoint};
pub use error::LlmError;
