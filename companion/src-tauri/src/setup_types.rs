use crate::settings::{Backend, CompanionSettings};
use crate::state::CompanionAppState;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum SetupStatus {
    Unknown,
    Checking,
    NotConfigured,
    Ready,
    Running,
    RepairRequired,
    Installing,
    Unsupported,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStepId {
    System,
    Python,
    Runtime,
    Dependencies,
    Port,
    Service,
    Model,
    Obsidian,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStepStatus {
    Pending,
    Running,
    Passed,
    Warning,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum SetupPrimaryAction {
    Setup,
    Repair,
    Start,
    Stop,
    Retry,
    None,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStep {
    pub id: SetupStepId,
    pub label: String,
    pub status: SetupStepStatus,
    pub summary: String,
    pub detail: Option<String>,
    pub recoverable: bool,
}

impl SetupStep {
    pub fn new(
        id: SetupStepId,
        label: impl Into<String>,
        status: SetupStepStatus,
        summary: impl Into<String>,
        recoverable: bool,
    ) -> Self {
        Self {
            id,
            label: label.into(),
            status,
            summary: summary.into(),
            detail: None,
            recoverable,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupResponse {
    pub status: SetupStatus,
    pub steps: Vec<SetupStep>,
    pub settings: CompanionSettings,
    pub state: CompanionAppState,
    pub primary_action: SetupPrimaryAction,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct PythonCandidate {
    pub path: String,
    pub valid: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SetupDetection {
    pub status: SetupStatus,
    pub steps: Vec<SetupStep>,
    pub settings: CompanionSettings,
    pub primary_action: SetupPrimaryAction,
    pub message: String,
    pub python_path: Option<String>,
    pub asr_service_path: Option<String>,
    pub python_candidates: Vec<PythonCandidate>,
    pub dependencies_ready: bool,
    pub port_available: bool,
    pub existing_service_healthy: bool,
}

impl SetupDetection {
    pub fn response(self, state: CompanionAppState) -> SetupResponse {
        SetupResponse {
            status: self.status,
            steps: self.steps,
            settings: self.settings,
            state,
            primary_action: self.primary_action,
            message: self.message,
        }
    }
}

pub fn backend_dependency_extra(backend: Backend) -> &'static str {
    match backend {
        Backend::Fake => ".",
        Backend::MlxAudio => ".[mlx]",
    }
}
