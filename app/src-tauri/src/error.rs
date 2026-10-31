//! Errors returned to the UI: a fixed code the UI translates, and a detail line. Neither ever
//! holds transcript text or keys.

use echonote_core::capture::CaptureError;
use echonote_core::engine::EngineError;
use echonote_core::store::StoreError;
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

impl CommandError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid", message)
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl From<StoreError> for CommandError {
    fn from(error: StoreError) -> Self {
        let code = match error {
            StoreError::NoMeeting(_)
            | StoreError::NoSegment(_)
            | StoreError::NoRevision { .. }
            | StoreError::NoGlossaryEntry(_)
            | StoreError::NoTemplate(_)
            | StoreError::NoCorrection(_) => "not-found",
            StoreError::TooNew { .. } => "library-too-new",
            StoreError::Glossary(_) | StoreError::InvalidTemplate => "invalid",
            StoreError::DuplicateTerm(_) => "duplicate-term",
            StoreError::GlossaryFull => "glossary-full",
            StoreError::TooManyTemplates => "too-many-templates",
            _ => "store",
        };
        Self::new(code, error.to_string())
    }
}

impl From<EngineError> for CommandError {
    fn from(error: EngineError) -> Self {
        let code = match &error {
            EngineError::Store(store) => return Self::new(store_code(store), error.to_string()),
            EngineError::Capture(CaptureError::Denied) => "mic-denied",
            EngineError::Capture(CaptureError::NoDevice(_)) => "no-device",
            EngineError::Capture(_) => "capture",
            EngineError::Busy => "busy",
            EngineError::Recovering => "recovering",
            EngineError::InUse => "meeting-in-use",
            EngineError::NoSession => "no-session",
            EngineError::NoInput => "no-input",
            EngineError::Detector(_) => "vad-missing",
            EngineError::Recording(_) | EngineError::Io(_) => "io",
            EngineError::Stopped => "engine",
        };
        Self::new(code, error.to_string())
    }
}

impl From<echonote_llm::LlmError> for CommandError {
    /// The message can hold the provider's own words; logs take the code only.
    fn from(error: echonote_llm::LlmError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

fn store_code(error: &StoreError) -> &'static str {
    match error {
        StoreError::TooNew { .. } => "library-too-new",
        _ => "store",
    }
}

pub type CommandResult<T> = Result<T, CommandError>;
