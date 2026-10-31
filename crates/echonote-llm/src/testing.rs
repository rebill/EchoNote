//! A scripted model for tests: answers come from a function of the request, a prompt over
//! `max_input` tokens is refused as too long, and every request is recorded.

use std::sync::{Arc, Mutex};

use crate::client::{Chat, Finish, Request};
use crate::{Cancel, LlmError, tokens};

/// What the fake was asked: the system prompt and the user turns.
#[derive(Clone, Debug)]
pub struct Asked {
    pub system: String,
    pub user: Vec<String>,
    pub max_tokens: u32,
    pub tokens: usize,
}

type Responder = dyn Fn(&Asked) -> Result<String, LlmError> + Send + Sync;

#[derive(Clone)]
pub struct FakeChat {
    responder: Arc<Responder>,
    pub max_input: usize,
    pub asked: Arc<Mutex<Vec<Asked>>>,
}

impl FakeChat {
    pub fn new(
        max_input: usize,
        responder: impl Fn(&Asked) -> Result<String, LlmError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            responder: Arc::new(responder),
            max_input,
            asked: Arc::default(),
        }
    }

    pub fn asked(&self) -> Vec<Asked> {
        self.asked.lock().unwrap().clone()
    }
}

impl Chat for FakeChat {
    async fn stream(
        &self,
        request: &Request,
        cancel: &Cancel,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<Finish, LlmError> {
        let user: Vec<String> = request.messages.iter().map(|m| m.content.clone()).collect();
        let size = tokens::estimate(&request.system)
            + user.iter().map(|u| tokens::estimate(u)).sum::<usize>();
        let asked = Asked {
            system: request.system.clone(),
            user,
            max_tokens: request.max_tokens,
            tokens: size,
        };
        self.asked.lock().unwrap().push(asked.clone());
        if cancel.is_cancelled() {
            return Err(LlmError::Cancelled);
        }
        if size > self.max_input {
            return Err(LlmError::ContextTooLong {
                detail: format!("{size} > {}", self.max_input),
            });
        }
        let reply = (self.responder)(&asked)?;
        // In two pieces, like a stream.
        let middle = reply
            .char_indices()
            .nth(reply.chars().count() / 2)
            .map_or(0, |(at, _)| at);
        on_text(&reply[..middle]);
        on_text(&reply[middle..]);
        Ok(Finish::default())
    }
}
