use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

const SETTINGS_FILE_NAME: &str = "companion-settings.json";
const DEFAULT_MODEL_QWEN3_0_6B: &str = "mlx-community/Qwen3-ASR-0.6B-4bit";
const DEFAULT_MODEL_QWEN3_1_7B: &str = "mlx-community/Qwen3-ASR-1.7B-4bit";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    Fake,
    MlxAudio,
}

impl Default for Backend {
    fn default() -> Self {
        Self::Fake
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
    pub custom_model_id: String,
    pub auto_start_service: bool,
}

impl Default for CompanionSettings {
    fn default() -> Self {
        Self {
            python_path: "python3".to_string(),
            asr_service_path: "../asr-service".to_string(),
            preferred_port: 8765,
            backend: Backend::Fake,
            model_preset: ModelPreset::Qwen3_0_6b4bit,
            custom_model_id: String::new(),
            auto_start_service: false,
        }
    }
}

impl CompanionSettings {
    pub fn normalized(mut self) -> Self {
        let defaults = Self::default();

        self.python_path = trimmed_or_default(self.python_path, defaults.python_path);
        self.asr_service_path =
            trimmed_or_default(self.asr_service_path, defaults.asr_service_path);
        self.custom_model_id = self.custom_model_id.trim().to_string();

        if self.preferred_port == 0 {
            self.preferred_port = defaults.preferred_port;
        }

        if self.model_preset == ModelPreset::Custom && self.custom_model_id.is_empty() {
            self.model_preset = defaults.model_preset;
        }

        self
    }

    pub fn resolved_model_id(&self) -> String {
        match self.model_preset {
            ModelPreset::Qwen3_0_6b4bit => DEFAULT_MODEL_QWEN3_0_6B.to_string(),
            ModelPreset::Qwen3_1_7b4bit => DEFAULT_MODEL_QWEN3_1_7B.to_string(),
            ModelPreset::Custom => {
                let custom_model_id = self.custom_model_id.trim();
                if custom_model_id.is_empty() {
                    DEFAULT_MODEL_QWEN3_0_6B.to_string()
                } else {
                    custom_model_id.to_string()
                }
            }
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
            Ok(settings) => {
                let normalized = settings.clone().normalized();
                if normalized != settings {
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

#[cfg(test)]
mod tests {
    use super::SettingsStore;
    use super::{Backend, CompanionSettings, ModelPreset};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolves_model_id_from_preset() {
        let settings = CompanionSettings::default();
        assert_eq!(
            settings.resolved_model_id(),
            "mlx-community/Qwen3-ASR-0.6B-4bit"
        );
    }

    #[test]
    fn resolves_custom_model_id() {
        let settings = CompanionSettings {
            model_preset: ModelPreset::Custom,
            custom_model_id: " custom/model ".to_string(),
            ..CompanionSettings::default()
        };

        assert_eq!(settings.resolved_model_id(), "custom/model");
    }

    #[test]
    fn normalizes_empty_required_values_to_defaults() {
        let settings = CompanionSettings {
            python_path: " ".to_string(),
            asr_service_path: "".to_string(),
            preferred_port: 0,
            backend: Backend::MlxAudio,
            model_preset: ModelPreset::Custom,
            custom_model_id: " ".to_string(),
            auto_start_service: true,
        }
        .normalized();

        assert_eq!(settings.python_path, "python3");
        assert_eq!(settings.asr_service_path, "../asr-service");
        assert_eq!(settings.preferred_port, 8765);
        assert_eq!(settings.backend, Backend::MlxAudio);
        assert_eq!(settings.model_preset, ModelPreset::Qwen3_0_6b4bit);
        assert!(settings.auto_start_service);
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
                custom_model_id: "local/model".to_string(),
                auto_start_service: true,
                ..CompanionSettings::default()
            })
            .expect("save settings");

        assert!(!saved.recovered);
        assert_eq!(saved.settings.preferred_port, 9001);
        assert_eq!(saved.settings.resolved_model_id(), "local/model");

        let reloaded = store.load_or_default().expect("reload settings");
        assert!(!reloaded.recovered);
        assert_eq!(reloaded.settings.python_path, "/usr/bin/python3");
        assert_eq!(reloaded.settings.backend, Backend::MlxAudio);

        fs::write(&path, "{not-json").expect("write invalid json");
        let recovered = store.load_or_default().expect("recover default settings");
        assert!(recovered.recovered);
        assert_eq!(recovered.settings, CompanionSettings::default());

        let _ = fs::remove_file(path);
    }

    fn temp_settings_path() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("echonote-companion-settings-test-{nonce}"))
            .join("companion-settings.json")
    }
}
