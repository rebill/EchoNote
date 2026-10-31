use std::time::Duration;

/// What went wrong, in terms the UI can explain (PRD A10). `detail` is the provider's own
/// error message, cut to [`MAX_DETAIL`] characters; it can echo request content, so log
/// [`LlmError::code`], not the message.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("the server address is not a valid http(s) URL")]
    InvalidUrl,
    #[error("plain http is only allowed for this computer")]
    InsecureUrl,
    #[error("the proxy address is not a valid http(s) URL")]
    InvalidProxy,
    #[error("a header name or value is not valid")]
    InvalidHeader,
    #[error("the HTTP client could not start")]
    Setup,
    #[error("the API key was rejected ({status}): {detail}")]
    Unauthorized { status: u16, detail: String },
    #[error("the account has no quota or balance left: {detail}")]
    Quota { detail: String },
    #[error("rate limited: {detail}")]
    RateLimited {
        retry_after: Option<Duration>,
        detail: String,
    },
    #[error("the model or address was not found: {detail}")]
    NotFound { detail: String },
    #[error("the transcript is too long for this model: {detail}")]
    ContextTooLong { detail: String },
    #[error("the request was rejected ({status}): {detail}")]
    Rejected { status: u16, detail: String },
    #[error("server error ({status}): {detail}")]
    Server { status: u16, detail: String },
    #[error("the server answered with a redirect; redirects are not followed")]
    Redirected,
    #[error("cannot connect to the server")]
    Connect,
    #[error("the server stopped responding")]
    Timeout,
    #[error("the answer stopped before it was complete")]
    Interrupted,
    #[error("the server's answer could not be read: {0}")]
    BadResponse(&'static str),
    #[error("cancelled")]
    Cancelled,
}

pub const MAX_DETAIL: usize = 300;

impl LlmError {
    /// A fixed name for logs and the UI's translations.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidUrl => "llm-invalid-url",
            Self::InsecureUrl => "llm-insecure-url",
            Self::InvalidProxy => "llm-invalid-proxy",
            Self::InvalidHeader => "llm-invalid-header",
            Self::Setup => "llm-setup",
            Self::Unauthorized { .. } => "llm-unauthorized",
            Self::Quota { .. } => "llm-quota",
            Self::RateLimited { .. } => "llm-rate-limited",
            Self::NotFound { .. } => "llm-not-found",
            Self::ContextTooLong { .. } => "llm-context-too-long",
            Self::Rejected { .. } => "llm-rejected",
            Self::Server { .. } => "llm-server",
            Self::Redirected => "llm-redirected",
            Self::Connect => "llm-connect",
            Self::Timeout => "llm-timeout",
            Self::Interrupted => "llm-interrupted",
            Self::BadResponse(_) => "llm-bad-response",
            Self::Cancelled => "cancelled",
        }
    }

    /// Worth trying again unchanged.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited { .. } | Self::Server { .. } | Self::Connect | Self::Timeout
        )
    }
}

pub(crate) fn detail(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = text.chars().take(MAX_DETAIL).collect();
    if text.chars().count() > MAX_DETAIL {
        out.push('…');
    }
    out
}
