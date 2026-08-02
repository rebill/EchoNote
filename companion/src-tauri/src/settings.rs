use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

const SETTINGS_FILE_NAME: &str = "companion-settings.json";
const OFFLINE_MODEL_NOT_INSTALLED: &str = "offline-asr-model-not-installed";
const DEFAULT_OFFLINE_BUNDLE_PATH: &str = "~/Library/Application Support/EchoNote/offline-bundle";
const DEFAULT_RUNTIME_PATH: &str = "~/Library/Application Support/EchoNote/runtime";
const DEFAULT_MODELS_PATH: &str = "~/Library/Application Support/EchoNote/models";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    Fake,
    MlxAudio,
}

impl Default for Backend {
    fn default() -> Self {
        Self::MlxAudio
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelPreset {
    #[serde(rename = "qwen3-0.6b-4bit")]
    Qwen3_0_6b4bit,
    #[serde(rename = "qwen3-1.7b-4bit")]
    Qwen3_1_7b4bit,
    #[serde(rename = "custom")]
    Custom,
}

impl Default for ModelPreset {
    fn default() -> Self {
        Self::Qwen3_0_6b4bit
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CompanionSettings {
    pub python_path: String,
    pub asr_service_path: String,
    pub preferred_port: u16,
    pub backend: Backend,
    pub model_preset: ModelPreset,
    #[serde(alias = "customModelId")]
    pub custom_model_path: String,
    pub offline_mode: bool,
    pub offline_bundle_path: String,
    pub runtime_path: String,
    pub models_path: String,
    pub asr_model_path: String,
    pub auto_start_service: bool,
    pub setup_completed_at: Option<String>,
    pub setup_version: Option<String>,
    pub auto_repair_enabled: bool,
    pub diarization_enabled: bool,
    #[serde(alias = "diarizationModelId")]
    pub diarization_model_path: String,
}

impl Default for CompanionSettings {
    fn default() -> Self {
        Self {
            python_path: "python3".to_string(),
            asr_service_path: "../asr-service".to_string(),
            preferred_port: 8765,
            backend: Backend::MlxAudio,
            model_preset: ModelPreset::Qwen3_0_6b4bit,
            custom_model_path: String::new(),
            offline_mode: true,
            offline_bundle_path: DEFAULT_OFFLINE_BUNDLE_PATH.to_string(),
            runtime_path: DEFAULT_RUNTIME_PATH.to_string(),
            models_path: DEFAULT_MODELS_PATH.to_string(),
            asr_model_path: String::new(),
            auto_start_service: false,
            setup_completed_at: None,
            setup_version: None,
            auto_repair_enabled: false,
            diarization_enabled: true,
            diarization_model_path: String::new(),
        }
    }
}

impl CompanionSettings {
    pub fn normalized(mut self) -> Self {
        let defaults = Self::default();

        self.python_path = trimmed_or_default(self.python_path, defaults.python_path);
        self.asr_service_path =
            trimmed_or_default(self.asr_service_path, defaults.asr_service_path);
        self.custom_model_path = self.custom_model_path.trim().to_string();
        self.offline_mode = true;
        self.offline_bundle_path =
            trimmed_or_default(self.offline_bundle_path, defaults.offline_bundle_path);
        self.runtime_path = trimmed_or_default(self.runtime_path, defaults.runtime_path);
        self.models_path = trimmed_or_default(self.models_path, defaults.models_path);
        self.asr_model_path = self.asr_model_path.trim().to_string();
        self.diarization_model_path = self.diarization_model_path.trim().to_string();
        self.setup_completed_at = self
            .setup_completed_at
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.setup_version = self
            .setup_version
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if self.preferred_port == 0 {
            self.preferred_port = defaults.preferred_port;
        }

        if self.model_preset == ModelPreset::Custom && self.custom_model_path.is_empty() {
            self.model_preset = defaults.model_preset;
        }

        self
    }

    pub fn resolved_model_id(&self) -> String {
        let configured = match self.model_preset {
            ModelPreset::Custom => self.custom_model_path.trim(),
            ModelPreset::Qwen3_0_6b4bit | ModelPreset::Qwen3_1_7b4bit => self.asr_model_path.trim(),
        };
        if configured.is_empty() {
            OFFLINE_MODEL_NOT_INSTALLED.to_string()
        } else {
            configured.to_string()
        }
    }

    pub fn selected_preset(&self) -> &'static str {
        match self.model_preset {
            ModelPreset::Qwen3_0_6b4bit => "qwen3-0.6b-4bit",
            ModelPreset::Qwen3_1_7b4bit => "qwen3-1.7b-4bit",
            ModelPreset::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsResponse {
    pub settings: CompanionSettings,
    pub settings_path: String,
    pub recovered: bool,
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    settings_path: PathBuf,
}

impl SettingsStore {
    pub fn new(app: &tauri::AppHandle) -> Result<Self, String> {
        let home_dir = app
            .path()
            .home_dir()
            .map_err(|error| format!("Failed to resolve home directory: {error}"))?;

        Ok(Self {
            settings_path: home_dir
                .join("Library")
                .join("Application Support")
                .join("EchoNote")
                .join(SETTINGS_FILE_NAME),
        })
    }

    #[cfg(test)]
    fn from_path(settings_path: PathBuf) -> Self {
        Self { settings_path }
    }

    pub fn load_or_default(&self) -> Result<SettingsResponse, String> {
        if !self.settings_path.exists() {
            return self.save_recovered_default();
        }

        let raw = match fs::read_to_string(&self.settings_path) {
            Ok(raw) => raw,
            Err(_) => return self.save_recovered_default(),
        };

        match serde_json::from_str::<CompanionSettings>(&raw) {
            Ok(mut settings) => {
                apply_legacy_migration(&raw, &mut settings);
                let normalized = settings.clone().normalized();
                if normalized != settings || contains_legacy_online_settings(&raw) {
                    self.write(&normalized)?;
                }
                Ok(self.response(normalized, false))
            }
            Err(_) => self.save_recovered_default(),
        }
    }

    pub fn save(&self, settings: CompanionSettings) -> Result<SettingsResponse, String> {
        let normalized = settings.normalized();
        self.write(&normalized)?;
        Ok(self.response(normalized, false))
    }

    pub fn load_read_only_or_default(&self) -> SettingsResponse {
        if !self.settings_path.exists() {
            return self.response(CompanionSettings::default(), true);
        }

        let Ok(raw) = fs::read_to_string(&self.settings_path) else {
            return self.response(CompanionSettings::default(), true);
        };

        match serde_json::from_str::<CompanionSettings>(&raw) {
            Ok(mut settings) => {
                apply_legacy_migration(&raw, &mut settings);
                self.response(settings.normalized(), false)
            }
            Err(_) => self.response(CompanionSettings::default(), true),
        }
    }

    pub fn settings_path_string(&self) -> String {
        self.settings_path.to_string_lossy().into_owned()
    }

    fn save_recovered_default(&self) -> Result<SettingsResponse, String> {
        let settings = CompanionSettings::default();
        self.write(&settings)?;
        Ok(self.response(settings, true))
    }

    fn write(&self, settings: &CompanionSettings) -> Result<(), String> {
        if let Some(parent) = self.settings_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create settings directory: {error}"))?;
        }

        let json = serde_json::to_string_pretty(settings)
            .map_err(|error| format!("Failed to serialize settings: {error}"))?;
        let temp_path = self.settings_path.with_extension("json.tmp");
        fs::write(&temp_path, json)
            .map_err(|error| format!("Failed to write settings file: {error}"))?;
        fs::rename(&temp_path, &self.settings_path)
            .map_err(|error| format!("Failed to replace settings file: {error}"))?;
        Ok(())
    }

    fn response(&self, settings: CompanionSettings, recovered: bool) -> SettingsResponse {
        SettingsResponse {
            settings,
            settings_path: self.settings_path.to_string_lossy().into_owned(),
            recovered,
        }
    }
}

fn trimmed_or_default(value: String, default_value: String) -> String {
    let value = value.trim();
    if value.is_empty() {
        default_value
    } else {
        value.to_string()
    }
}

fn contains_legacy_online_settings(raw: &str) -> bool {
    raw.contains("\"huggingFaceToken\"")
        || raw.contains("\"customModelId\"")
        || raw.contains("\"diarizationModelId\"")
}

fn apply_legacy_migration(raw: &str, settings: &mut CompanionSettings) {
    if raw.contains("\"customModelId\"") {
        settings.custom_model_path.clear();
    }
    if raw.contains("\"diarizationModelId\"") {
        settings.diarization_model_path.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::SettingsStore;
    use super::{Backend, CompanionSettings, ModelPreset};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reports_uninstalled_offline_model_by_default() {
        let settings = CompanionSettings::default();
        assert_eq!(
            settings.resolved_model_id(),
            "offline-asr-model-not-installed"
        );
    }

    #[test]
    fn resolves_custom_local_model_path() {
        let settings = CompanionSettings {
            model_preset: ModelPreset::Custom,
            custom_model_path: " /models/custom ".to_string(),
            ..CompanionSettings::default()
        };

        assert_eq!(settings.resolved_model_id(), "/models/custom");
    }

    #[test]
    fn normalizes_empty_required_values_to_defaults() {
        let settings = CompanionSettings {
            python_path: " ".to_string(),
            asr_service_path: "".to_string(),
            preferred_port: 0,
            backend: Backend::MlxAudio,
            model_preset: ModelPreset::Custom,
            custom_model_path: " ".to_string(),
            offline_mode: false,
            offline_bundle_path: " ".to_string(),
            runtime_path: " ".to_string(),
            models_path: " ".to_string(),
            asr_model_path: " /models/asr ".to_string(),
            auto_start_service: true,
            setup_completed_at: Some(" ".to_string()),
            setup_version: Some(" 0.3.0 ".to_string()),
            auto_repair_enabled: true,
            diarization_enabled: true,
            diarization_model_path: " /models/diarization ".to_string(),
        }
        .normalized();

        assert_eq!(settings.python_path, "python3");
        assert_eq!(settings.asr_service_path, "../asr-service");
        assert_eq!(settings.preferred_port, 8765);
        assert_eq!(settings.backend, Backend::MlxAudio);
        assert_eq!(settings.model_preset, ModelPreset::Qwen3_0_6b4bit);
        assert!(settings.offline_mode);
        assert_eq!(
            settings.offline_bundle_path,
            "~/Library/Application Support/EchoNote/offline-bundle"
        );
        assert_eq!(
            settings.runtime_path,
            "~/Library/Application Support/EchoNote/runtime"
        );
        assert_eq!(
            settings.models_path,
            "~/Library/Application Support/EchoNote/models"
        );
        assert_eq!(settings.asr_model_path, "/models/asr");
        assert!(settings.auto_start_service);
        assert_eq!(settings.setup_completed_at, None);
        assert_eq!(settings.setup_version.as_deref(), Some("0.3.0"));
        assert!(settings.auto_repair_enabled);
        assert!(settings.diarization_enabled);
        assert_eq!(settings.diarization_model_path, "/models/diarization");
    }

    #[test]
    fn persists_settings_and_recovers_invalid_json() {
        let path = temp_settings_path();
        let store = SettingsStore::from_path(path.clone());

        let initial = store.load_or_default().expect("load default settings");
        assert!(initial.recovered);
        assert!(path.exists());

        let saved = store
            .save(CompanionSettings {
                python_path: "/usr/bin/python3".to_string(),
                preferred_port: 9001,
                backend: Backend::MlxAudio,
                model_preset: ModelPreset::Custom,
                custom_model_path: "/models/local".to_string(),
                auto_start_service: true,
                setup_completed_at: Some("2026-05-21T00:00:00Z".to_string()),
                setup_version: Some("0.3.0".to_string()),
                auto_repair_enabled: true,
                diarization_enabled: true,
                diarization_model_path: "/models/pyannote".to_string(),
                ..CompanionSettings::default()
            })
            .expect("save settings");

        assert!(!saved.recovered);
        assert_eq!(saved.settings.preferred_port, 9001);
        assert_eq!(saved.settings.resolved_model_id(), "/models/local");

        let reloaded = store.load_or_default().expect("reload settings");
        assert!(!reloaded.recovered);
        assert_eq!(reloaded.settings.python_path, "/usr/bin/python3");
        assert_eq!(reloaded.settings.backend, Backend::MlxAudio);
        assert_eq!(reloaded.settings.setup_version.as_deref(), Some("0.3.0"));
        assert_eq!(reloaded.settings.diarization_model_path, "/models/pyannote");

        fs::write(&path, "{not-json").expect("write invalid json");
        let recovered = store.load_or_default().expect("recover default settings");
        assert!(recovered.recovered);
        assert_eq!(recovered.settings, CompanionSettings::default());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn removes_legacy_token_and_remote_model_fields_during_migration() {
        let path = temp_settings_path();
        fs::create_dir_all(path.parent().expect("settings parent"))
            .expect("create settings parent");
        fs::write(
            &path,
            r#"{
  "backend": "mlx-audio",
  "huggingFaceToken": "hf_secret",
  "customModelId": "mlx-community/remote-model",
  "diarizationModelId": "pyannote/remote-model"
}"#,
        )
        .expect("write legacy settings");
        let store = SettingsStore::from_path(path.clone());

        let migrated = store.load_or_default().expect("migrate settings");
        let persisted = fs::read_to_string(&path).expect("read migrated settings");

        assert!(!migrated.recovered);
        assert!(!persisted.contains("huggingFaceToken"));
        assert!(!persisted.contains("customModelId"));
        assert!(!persisted.contains("diarizationModelId"));
        assert!(persisted.contains("offlineMode"));
        let _ = fs::remove_dir_all(path.parent().expect("settings parent"));
    }

    fn temp_settings_path() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("echonote-settings-test-{nonce}"))
            .join("companion-settings.json")
    }
}
