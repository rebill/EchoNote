//! Streaming chat over HTTP. One request at a time per call; `stream` retries rate limits,
//! server errors, timeouts and refused connections while no text has been handed out, and
//! reports a stream that breaks after that as `Interrupted` (the caller already showed it).

use std::future::Future;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};

use crate::error::detail;
use crate::sse::{Event, Parser};
use crate::{Api, Cancel, Endpoint, LlmError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// One chat request. Not `Debug`: it holds transcript text.
#[derive(Clone)]
pub struct Request {
    pub system: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    /// `None` leaves the server's default (some models accept no other value).
    pub temperature: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Finish {
    /// The model stopped at `max_tokens`.
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub connect_timeout: Duration,
    /// Longest wait for the response to start or for the next chunk. Local models can take
    /// a while to load and read a long prompt.
    pub idle_timeout: Duration,
    pub attempts: u32,
    pub first_backoff: Duration,
    /// Longest wait between attempts, also for a server's `retry-after`.
    pub max_backoff: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            idle_timeout: Duration::from_secs(180),
            attempts: 3,
            first_backoff: Duration::from_secs(2),
            max_backoff: Duration::from_secs(30),
        }
    }
}

/// What the features need from a model: text streamed to `on_text`.
pub trait Chat: Sync {
    fn stream(
        &self,
        request: &Request,
        cancel: &Cancel,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> impl Future<Output = Result<Finish, LlmError>> + Send;
}

/// The whole answer at once.
pub async fn complete<C: Chat>(
    chat: &C,
    request: &Request,
    cancel: &Cancel,
) -> Result<(String, Finish), LlmError> {
    let mut text = String::new();
    let finish = chat
        .stream(request, cancel, &mut |chunk| text.push_str(chunk))
        .await?;
    Ok((text, finish))
}

/// Parameters some OpenAI-compatible models refuse; learned from a 400 and dropped.
#[derive(Clone, Copy, Debug, Default)]
struct Quirks {
    max_completion_tokens: bool,
    no_temperature: bool,
}

pub struct Client {
    endpoint: Endpoint,
    http: reqwest::Client,
    policy: Policy,
}

/// Request headers nobody may set by hand.
const RESERVED_HEADERS: [&str; 4] = ["host", "content-length", "transfer-encoding", "connection"];
/// Error bodies are read up to this size.
const MAX_ERROR_BODY: usize = 16 * 1024;
/// A non-streamed answer, or the model list.
const MAX_BODY: usize = 8 << 20;

impl Client {
    pub fn new(endpoint: Endpoint, policy: Policy) -> Result<Self, LlmError> {
        endpoint.url("")?;
        let mut builder = reqwest::Client::builder()
            .user_agent(concat!("EchoNote/", env!("CARGO_PKG_VERSION")))
            // A redirect would carry the key to wherever it points.
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(policy.connect_timeout);
        if endpoint.is_local() {
            builder = builder.no_proxy();
        } else if let Some(proxy) = endpoint
            .proxy
            .as_deref()
            .filter(|proxy| !proxy.trim().is_empty())
        {
            let url = url::Url::parse(proxy.trim()).map_err(|_| LlmError::InvalidProxy)?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err(LlmError::InvalidProxy);
            }
            builder = builder
                .proxy(reqwest::Proxy::all(url.as_str()).map_err(|_| LlmError::InvalidProxy)?);
        }
        let http = builder.build().map_err(|_| LlmError::Setup)?;
        Ok(Self {
            endpoint,
            http,
            policy,
        })
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}

fn secret(value: &str) -> Result<HeaderValue, LlmError> {
    let mut value = HeaderValue::from_str(value).map_err(|_| LlmError::InvalidHeader)?;
    value.set_sensitive(true);
    Ok(value)
}

impl Client {
    fn headers(&self) -> Result<HeaderMap, LlmError> {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        let key = self
            .endpoint
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty());
        match self.endpoint.api {
            Api::OpenAi => {
                if let Some(key) = key {
                    headers.insert("authorization", secret(&format!("Bearer {key}"))?);
                }
            }
            Api::Anthropic => {
                headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
                if let Some(key) = key {
                    headers.insert("x-api-key", secret(key)?);
                }
            }
        }
        for (name, value) in &self.endpoint.headers {
            let name = HeaderName::from_bytes(name.trim().as_bytes())
                .map_err(|_| LlmError::InvalidHeader)?;
            if RESERVED_HEADERS.contains(&name.as_str()) {
                return Err(LlmError::InvalidHeader);
            }
            headers.insert(name, secret(value.trim())?);
        }
        Ok(headers)
    }

    fn body(&self, request: &Request, quirks: Quirks) -> Value {
        let messages = request.messages.iter().map(|message| {
            json!({
                "role": match message.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                "content": message.content,
            })
        });
        let temperature = request.temperature.filter(|_| !quirks.no_temperature);
        match self.endpoint.api {
            Api::OpenAi => {
                let mut all = Vec::new();
                if !request.system.is_empty() {
                    all.push(json!({"role": "system", "content": request.system}));
                }
                all.extend(messages);
                let mut body = json!({
                    "model": self.endpoint.model,
                    "messages": all,
                    "stream": true,
                });
                let limit = if quirks.max_completion_tokens {
                    "max_completion_tokens"
                } else {
                    "max_tokens"
                };
                body[limit] = json!(request.max_tokens);
                if let Some(temperature) = temperature {
                    body["temperature"] = json!(temperature);
                }
                body
            }
            Api::Anthropic => {
                let mut body = json!({
                    "model": self.endpoint.model,
                    "max_tokens": request.max_tokens,
                    "messages": messages.collect::<Vec<_>>(),
                    "stream": true,
                });
                if !request.system.is_empty() {
                    body["system"] = json!(request.system);
                }
                if let Some(temperature) = temperature {
                    body["temperature"] = json!(temperature);
                }
                body
            }
        }
    }

    fn chat_url(&self) -> Result<url::Url, LlmError> {
        self.endpoint.url(match self.endpoint.api {
            Api::OpenAi => "chat/completions",
            Api::Anthropic => "messages",
        })
    }
}

impl Client {
    /// `future`, unless cancelled or idle for longer than the policy allows.
    async fn wait<T>(
        &self,
        cancel: &Cancel,
        future: impl Future<Output = T>,
    ) -> Result<T, LlmError> {
        unless_cancelled(
            cancel,
            tokio::time::timeout(self.policy.idle_timeout, future),
        )
        .await?
        .map_err(|_| LlmError::Timeout)
    }

    /// Send `body` to `url` and return the response once it is a success.
    async fn send(
        &self,
        method: reqwest::Method,
        url: url::Url,
        body: Option<Vec<u8>>,
        cancel: &Cancel,
    ) -> Result<reqwest::Response, LlmError> {
        let mut request = self.http.request(method, url).headers(self.headers()?);
        if let Some(body) = body {
            request = request.body(body);
        }
        let response = self
            .wait(cancel, request.send())
            .await?
            .map_err(transport)?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        if status.is_redirection() {
            return Err(LlmError::Redirected);
        }
        let retry_after = retry_after(response.headers());
        let body = self.read(response, MAX_ERROR_BODY, false, cancel).await?;
        Err(classify(
            status.as_u16(),
            retry_after,
            &String::from_utf8_lossy(&body),
        ))
    }

    /// The body up to `limit` bytes; past it, an error when `strict`, else cut off.
    async fn read(
        &self,
        response: reqwest::Response,
        limit: usize,
        strict: bool,
        cancel: &Cancel,
    ) -> Result<Vec<u8>, LlmError> {
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        while let Some(chunk) = self.wait(cancel, stream.next()).await? {
            let chunk = chunk.map_err(transport)?;
            if body.len() + chunk.len() > limit {
                if strict {
                    return Err(LlmError::BadResponse("the answer is too large"));
                }
                body.extend_from_slice(&chunk[..limit - body.len()]);
                break;
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

/// `future`'s output, or `Cancelled` as soon as `cancel` fires (checked first).
async fn unless_cancelled<T>(
    cancel: &Cancel,
    future: impl Future<Output = T>,
) -> Result<T, LlmError> {
    let cancelled = std::pin::pin!(cancel.cancelled());
    let future = std::pin::pin!(future);
    match futures_util::future::select(cancelled, future).await {
        futures_util::future::Either::Left(_) => Err(LlmError::Cancelled),
        futures_util::future::Either::Right((value, _)) => Ok(value),
    }
}

fn transport(error: reqwest::Error) -> LlmError {
    if error.is_timeout() {
        LlmError::Timeout
    } else if error.is_redirect() {
        LlmError::Redirected
    } else if error.is_connect() {
        LlmError::Connect
    } else if error.is_body() || error.is_decode() {
        LlmError::Interrupted
    } else {
        LlmError::Connect
    }
}

/// Words providers use when the prompt does not fit.
const CONTEXT_WORDS: [&str; 7] = [
    "context length",
    "context_length",
    "context window",
    "maximum context",
    "prompt is too long",
    "too many tokens",
    "range of input length",
];

fn classify(status: u16, retry_after: Option<Duration>, body: &str) -> LlmError {
    let message = error_message(body);
    let lower = message.to_lowercase();
    let detail = detail(&message);
    let quota = ["quota", "balance", "billing", "insufficient", "余额"]
        .iter()
        .any(|word| lower.contains(word));
    let context = CONTEXT_WORDS.iter().any(|word| lower.contains(word));
    match status {
        401 | 403 => LlmError::Unauthorized { status, detail },
        402 => LlmError::Quota { detail },
        429 if quota => LlmError::Quota { detail },
        429 => LlmError::RateLimited {
            retry_after,
            detail,
        },
        404 => LlmError::NotFound { detail },
        413 => LlmError::ContextTooLong { detail },
        400 | 422 if context => LlmError::ContextTooLong { detail },
        400..=499 => LlmError::Rejected { status, detail },
        _ => LlmError::Server { status, detail },
    }
}

/// The message from an OpenAI, Anthropic or Ollama error body, else the body itself.
fn error_message(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return body.trim().to_owned();
    };
    let error = value.get("error").unwrap_or(&value);
    let text = |value: &Value, key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    let message = match error {
        Value::String(message) => Some(message.clone()),
        _ => text(error, "message")
            .or_else(|| text(error, "detail"))
            .or_else(|| text(&value, "message")),
    };
    let code = text(error, "code").or_else(|| text(error, "type"));
    match (message, code) {
        (Some(message), Some(code)) if !message.contains(&code) => format!("{message} ({code})"),
        (Some(message), _) => message,
        (None, Some(code)) => code,
        (None, None) => body.trim().to_owned(),
    }
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let number = |name: &str| {
        headers
            .get(name)?
            .to_str()
            .ok()?
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value >= 0.0)
    };
    if let Some(ms) = number("retry-after-ms") {
        return Some(Duration::from_secs_f64(ms.min(3_600_000.0) / 1000.0));
    }
    number("retry-after").map(|seconds| Duration::from_secs_f64(seconds.min(3_600.0)))
}

/// Hides `<think>…</think>` that some local models put in the answer text.
#[derive(Default)]
struct ThinkFilter {
    held: String,
    thinking: bool,
}

impl ThinkFilter {
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";

    fn push(&mut self, chunk: &str) -> String {
        self.held.push_str(chunk);
        let mut out = String::new();
        loop {
            let tag = if self.thinking {
                Self::CLOSE
            } else {
                Self::OPEN
            };
            if let Some(at) = self.held.find(tag) {
                if !self.thinking {
                    out.push_str(&self.held[..at]);
                }
                self.held.drain(..at + tag.len());
                self.thinking = !self.thinking;
                continue;
            }
            // Keep a tail that could be the start of the tag.
            let keep = (1..tag.len())
                .rev()
                .find(|&n| self.held.ends_with(&tag[..n]))
                .unwrap_or(0);
            let cut = self.held.len() - keep;
            if !self.thinking {
                out.push_str(&self.held[..cut]);
            }
            self.held.drain(..cut);
            return out;
        }
    }

    fn finish(&mut self) -> String {
        if self.thinking {
            String::new()
        } else {
            std::mem::take(&mut self.held)
        }
    }
}

/// Turns one API's events into text.
struct Decoder {
    api: Api,
    done: bool,
    finished: bool,
    truncated: bool,
    think: ThinkFilter,
}

impl Decoder {
    fn new(api: Api) -> Self {
        Self {
            api,
            done: false,
            finished: false,
            truncated: false,
            think: ThinkFilter::default(),
        }
    }

    fn event(&mut self, event: &Event) -> Result<String, LlmError> {
        if self.api == Api::OpenAi && event.data.trim() == "[DONE]" {
            self.done = true;
            return Ok(self.think.finish());
        }
        let value: Value = serde_json::from_str(&event.data)
            .map_err(|_| LlmError::BadResponse("an event is not JSON"))?;
        if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
            return Err(stream_error(error));
        }
        let text = match self.api {
            Api::OpenAi => {
                let choice = value.get("choices").and_then(|choices| choices.get(0));
                if let Some(reason) = choice
                    .and_then(|choice| choice.get("finish_reason"))
                    .and_then(Value::as_str)
                {
                    self.finished = true;
                    self.truncated = reason == "length";
                }
                choice
                    .and_then(|choice| choice.pointer("/delta/content"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned()
            }
            Api::Anthropic => match value.get("type").and_then(Value::as_str) {
                Some("content_block_delta") => value
                    .pointer("/delta/text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                Some("message_delta") => {
                    let reason = value.pointer("/delta/stop_reason").and_then(Value::as_str);
                    self.truncated = reason == Some("max_tokens");
                    String::new()
                }
                Some("message_stop") => {
                    self.done = true;
                    return Ok(self.think.finish());
                }
                _ => String::new(),
            },
        };
        Ok(self.think.push(&text))
    }

    /// At the end of the body: complete only with the API's end marker (OpenAI servers that
    /// skip `[DONE]` still send a finish reason).
    fn end(&mut self) -> Result<(String, Finish), LlmError> {
        if self.done || self.finished {
            return Ok((
                self.think.finish(),
                Finish {
                    truncated: self.truncated,
                },
            ));
        }
        Err(LlmError::Interrupted)
    }
}

/// An error object inside a 200 response or an event stream.
fn stream_error(error: &Value) -> LlmError {
    let kind = error
        .get("type")
        .or_else(|| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let detail = detail(&error_message(&error.to_string()));
    match kind {
        "overloaded_error" | "api_error" | "server_error" => LlmError::Server { status: 0, detail },
        "rate_limit_error" | "rate_limit_exceeded" => LlmError::RateLimited {
            retry_after: None,
            detail,
        },
        "authentication_error" | "permission_error" | "invalid_api_key" => {
            LlmError::Unauthorized { status: 0, detail }
        }
        "insufficient_quota" => LlmError::Quota { detail },
        _ => LlmError::Rejected { status: 0, detail },
    }
}

/// A server that ignored `stream: true` and answered with one JSON body.
fn whole(api: Api, body: &[u8]) -> Result<(String, Finish), LlmError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| LlmError::BadResponse("the answer is not JSON"))?;
    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        return Err(stream_error(error));
    }
    let (text, truncated) = match api {
        Api::OpenAi => {
            let choice = value.pointer("/choices/0");
            let text = choice
                .and_then(|choice| choice.pointer("/message/content"))
                .and_then(Value::as_str);
            let reason = choice
                .and_then(|choice| choice.get("finish_reason"))
                .and_then(Value::as_str);
            (text.map(str::to_owned), reason == Some("length"))
        }
        Api::Anthropic => {
            let text = value
                .get("content")
                .and_then(Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|block| block.get("text").and_then(Value::as_str))
                        .collect::<String>()
                });
            let reason = value.get("stop_reason").and_then(Value::as_str);
            (text, reason == Some("max_tokens"))
        }
    };
    let text = text.ok_or(LlmError::BadResponse("the answer has no text"))?;
    let mut think = ThinkFilter::default();
    let mut text = think.push(&text);
    text.push_str(&think.finish());
    Ok((text, Finish { truncated }))
}

impl Client {
    /// One request. `emitted` is set once any text went to `on_text`.
    async fn attempt(
        &self,
        request: &Request,
        quirks: Quirks,
        cancel: &Cancel,
        on_text: &mut (dyn FnMut(&str) + Send),
        emitted: &mut bool,
    ) -> Result<Finish, LlmError> {
        let body = serde_json::to_vec(&self.body(request, quirks))
            .map_err(|_| LlmError::BadResponse("the request could not be encoded"))?;
        let response = self
            .send(reqwest::Method::POST, self.chat_url()?, Some(body), cancel)
            .await?;
        let event_stream = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"));
        if !event_stream {
            let body = self.read(response, MAX_BODY, true, cancel).await?;
            let (text, finish) = whole(self.endpoint.api, &body)?;
            if !text.is_empty() {
                *emitted = true;
                on_text(&text);
            }
            return Ok(finish);
        }
        let mut decoder = Decoder::new(self.endpoint.api);
        let mut parser = Parser::default();
        let mut stream = response.bytes_stream();
        let mut emit = |text: String, emitted: &mut bool| {
            if !text.is_empty() {
                *emitted = true;
                on_text(&text);
            }
        };
        while let Some(chunk) = self.wait(cancel, stream.next()).await? {
            let chunk = chunk.map_err(|error| match transport(error) {
                LlmError::Connect => LlmError::Interrupted,
                other => other,
            })?;
            for event in parser.feed(&chunk).map_err(LlmError::BadResponse)? {
                let text = decoder.event(&event)?;
                emit(text, emitted);
                if decoder.done {
                    return Ok(Finish {
                        truncated: decoder.truncated,
                    });
                }
            }
        }
        let (text, finish) = decoder.end()?;
        emit(text, emitted);
        Ok(finish)
    }

    /// Model ids the server offers (`GET {base}/models`), sorted.
    pub async fn models(&self, cancel: &Cancel) -> Result<Vec<String>, LlmError> {
        let response = self
            .send(
                reqwest::Method::GET,
                self.endpoint.url("models")?,
                None,
                cancel,
            )
            .await?;
        let body = self.read(response, MAX_BODY, true, cancel).await?;
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| LlmError::BadResponse("the model list is not JSON"))?;
        let list = value
            .get("data")
            .or_else(|| value.get("models"))
            .and_then(Value::as_array)
            .ok_or(LlmError::BadResponse("the model list has no entries"))?;
        let mut ids: Vec<String> = list
            .iter()
            .filter_map(|model| {
                model
                    .get("id")
                    .or_else(|| model.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    /// A tiny request with the configured model: key, address, model and proxy all work.
    pub async fn test(&self, cancel: &Cancel) -> Result<(), LlmError> {
        let request = Request {
            system: String::new(),
            messages: vec![Message::user("Reply with OK.")],
            max_tokens: 16,
            temperature: None,
        };
        self.stream(&request, cancel, &mut |_| {}).await.map(drop)
    }
}

impl Chat for Client {
    async fn stream(
        &self,
        request: &Request,
        cancel: &Cancel,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<Finish, LlmError> {
        let mut quirks = Quirks::default();
        let mut backoff = self.policy.first_backoff;
        let mut attempt = 1;
        loop {
            let mut emitted = false;
            let error = match self
                .attempt(request, quirks, cancel, on_text, &mut emitted)
                .await
            {
                Ok(finish) => return Ok(finish),
                Err(error) => error,
            };
            if emitted {
                // The caller has part of an answer; starting over would repeat it.
                return Err(match error {
                    LlmError::Cancelled => LlmError::Cancelled,
                    _ => LlmError::Interrupted,
                });
            }
            // Each workaround once: newer OpenAI models want `max_completion_tokens`, and some
            // only take the default temperature.
            if let LlmError::Rejected {
                status: 400,
                detail,
            } = &error
            {
                let lower = detail.to_lowercase();
                if lower.contains("max_tokens")
                    && self.endpoint.api == Api::OpenAi
                    && !quirks.max_completion_tokens
                {
                    quirks.max_completion_tokens = true;
                    continue;
                }
                if lower.contains("temperature")
                    && request.temperature.is_some()
                    && !quirks.no_temperature
                {
                    quirks.no_temperature = true;
                    continue;
                }
            }
            if !error.is_retryable() || attempt >= self.policy.attempts {
                return Err(error);
            }
            let wait = match &error {
                LlmError::RateLimited {
                    retry_after: Some(after),
                    ..
                } => *after,
                _ => backoff,
            }
            .min(self.policy.max_backoff);
            unless_cancelled(cancel, tokio::time::sleep(wait)).await?;
            backoff = (backoff * 2).min(self.policy.max_backoff);
            attempt += 1;
        }
    }
}
