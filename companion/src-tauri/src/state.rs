use crate::settings::{Backend, CompanionSettings};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum ServiceStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum ModelStatus {
    NotLoaded,
    Loading,
    Ready,
    Error,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionAppState {
    service_status: ServiceStatus,
    model_status: ModelStatus,
    base_url: Option<String>,
    pid: Option<u32>,
    resolved_model_id: String,
    backend: Backend,
    last_error: Option<String>,
    last_exit_code: Option<i32>,
    recent_logs: Vec<String>,
    discovery_path: String,
    settings_path: String,
    logs_path: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeState {
    pub service_status: ServiceStatus,
    pub model_status: ModelStatus,
    pub base_url: Option<String>,
    pub port: u16,
    pub pid: Option<u32>,
    pub resolved_model_id: String,
    pub backend: Backend,
    pub last_error: Option<String>,
    pub last_exit_code: Option<i32>,
    pub recent_logs: Vec<String>,
}

impl RuntimeState {
    pub fn from_settings(settings: &CompanionSettings) -> Self {
        Self {
            service_status: ServiceStatus::Stopped,
            model_status: ModelStatus::Unknown,
            base_url: Some(format!("http://127.0.0.1:{}", settings.preferred_port)),
            port: settings.preferred_port,
            pid: None,
            resolved_model_id: settings.resolved_model_id(),
            backend: settings.backend,
            last_error: None,
            last_exit_code: None,
            recent_logs: vec![
                "Companion backend scaffold loaded.".to_string(),
                "Process manager is idle.".to_string(),
            ],
        }
    }
}

impl CompanionAppState {
    pub(crate) fn from_runtime(
        settings_path: String,
        discovery_path: String,
        logs_path: String,
        runtime: RuntimeState,
    ) -> Self {
        Self {
            service_status: runtime.service_status,
            model_status: runtime.model_status,
            base_url: runtime.base_url,
            pid: runtime.pid,
            resolved_model_id: runtime.resolved_model_id,
            backend: runtime.backend,
            last_error: runtime.last_error,
            last_exit_code: runtime.last_exit_code,
            recent_logs: runtime.recent_logs,
            discovery_path,
            settings_path,
            logs_path,
        }
    }
}
