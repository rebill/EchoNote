use crate::settings::Backend;
use crate::state::{DiarizationStatus, ModelStatus, RuntimeState, ServiceStatus};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use tauri::Manager;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const DISCOVERY_FILE_NAME: &str = "companion.json";
const APP_NAME: &str = "EchoNote";
const SERVICE_NAME: &str = "echonote-asr";
const HOST: &str = "127.0.0.1";

#[derive(Debug, Clone)]
pub struct DiscoveryWriter {
    discovery_path: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompanionDiscovery {
    version: u8,
    app: &'static str,
    service: &'static str,
    status: ServiceStatus,
    base_url: String,
    host: &'static str,
    port: u16,
    backend: Backend,
    model_id: String,
    model_status: ModelStatus,
    pid: Option<u32>,
    updated_at: String,
    capabilities: CompanionCapabilities,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompanionCapabilities {
    adaptive_chunking: bool,
    speaker_diarization: DiarizationStatus,
}

impl DiscoveryWriter {
    pub fn new(app: &tauri::AppHandle) -> Result<Self, String> {
        let home_dir = app
            .path()
            .home_dir()
            .map_err(|error| format!("Failed to resolve home directory: {error}"))?;

        Ok(Self::from_path(
            home_dir
                .join("Library")
                .join("Application Support")
                .join("EchoNote")
                .join(DISCOVERY_FILE_NAME),
        ))
    }

    fn from_path(discovery_path: PathBuf) -> Self {
        Self { discovery_path }
    }

    pub fn path_string(&self) -> String {
        self.discovery_path.to_string_lossy().into_owned()
    }

    pub fn write(&self, runtime: &RuntimeState) -> Result<(), String> {
        let discovery = CompanionDiscovery::from_runtime(runtime)?;
        let json = serde_json::to_string_pretty(&discovery)
            .map_err(|error| format!("Failed to serialize discovery file: {error}"))?;

        if let Some(parent) = self.discovery_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create discovery directory: {error}"))?;
        }

        let temp_path = self.discovery_path.with_extension("json.tmp");
        fs::write(&temp_path, json)
            .map_err(|error| format!("Failed to write discovery temp file: {error}"))?;
        fs::rename(&temp_path, &self.discovery_path)
            .map_err(|error| format!("Failed to replace discovery file: {error}"))?;
        Ok(())
    }
}

impl CompanionDiscovery {
    fn from_runtime(runtime: &RuntimeState) -> Result<Self, String> {
        let base_url = runtime
            .base_url
            .clone()
            .unwrap_or_else(|| format!("http://{HOST}:{}", runtime.port));

        Ok(Self {
            version: 1,
            app: APP_NAME,
            service: SERVICE_NAME,
            status: runtime.service_status,
            base_url,
            host: HOST,
            port: runtime.port,
            backend: runtime.backend,
            model_id: runtime.resolved_model_id.clone(),
            model_status: runtime.model_status,
            pid: runtime.pid,
            updated_at: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .map_err(|error| format!("Failed to format discovery timestamp: {error}"))?,
            capabilities: CompanionCapabilities {
                adaptive_chunking: true,
                speaker_diarization: runtime.diarization_status,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::DiscoveryWriter;
    use crate::settings::CompanionSettings;
    use crate::state::{ModelStatus, RuntimeState, ServiceStatus};
    use serde_json::Value;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_schema_compatible_discovery_file_atomically() {
        let path = temp_discovery_path();
        let writer = DiscoveryWriter::from_path(path.clone());
        let settings = CompanionSettings::default();
        let mut runtime = RuntimeState::from_settings(&settings);
        runtime.service_status = ServiceStatus::Running;
        runtime.model_status = ModelStatus::NotLoaded;
        runtime.pid = Some(1234);

        writer.write(&runtime).expect("write discovery file");

        let raw = fs::read_to_string(&path).expect("read discovery file");
        let discovery: Value = serde_json::from_str(&raw).expect("parse discovery json");
        assert_eq!(discovery["version"], 1);
        assert_eq!(discovery["app"], "EchoNote");
        assert_eq!(discovery["service"], "echonote-asr");
        assert_eq!(discovery["status"], "running");
        assert_eq!(discovery["baseUrl"], "http://127.0.0.1:8765");
        assert_eq!(discovery["host"], "127.0.0.1");
        assert_eq!(discovery["port"], 8765);
        assert_eq!(discovery["backend"], "fake");
        assert_eq!(discovery["modelId"], "mlx-community/Qwen3-ASR-0.6B-4bit");
        assert_eq!(discovery["modelStatus"], "not_loaded");
        assert_eq!(discovery["pid"], 1234);
        assert_eq!(discovery["capabilities"]["adaptiveChunking"], true);
        assert_eq!(
            discovery["capabilities"]["speakerDiarization"],
            "unavailable"
        );
        assert!(discovery["updatedAt"]
            .as_str()
            .unwrap_or_default()
            .ends_with('Z'));
        assert!(!path.with_extension("json.tmp").exists());

        let _ = fs::remove_dir_all(path.parent().expect("temp discovery parent"));
    }

    fn temp_discovery_path() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("echonote-discovery-test-{nonce}"))
            .join("companion.json")
    }
}
