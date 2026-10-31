//! App settings, stored as one JSON value in the library's `settings` table so they move with
//! the library. Unknown or invalid fields fall back to their defaults.

use std::str::FromStr;
use std::time::Duration;

use echonote_core::store::Store;
use serde::{Deserialize, Serialize};
use tauri_plugin_global_shortcut::Shortcut;

use crate::error::CommandError;

pub const KEY: &str = "app";
pub const LANGUAGES: [&str; 2] = ["zh-CN", "en"];
/// Longest auto-stop the UI offers, in minutes.
pub const MAX_AUTO_STOP_MINUTES: u32 = 240;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// UI language: `zh-CN` or `en`.
    pub language: String,
    /// Microphone device id; `None` follows the system default.
    pub mic_device: Option<String>,
    /// Record system audio (the other side of a call) next to the microphone.
    pub record_system: bool,
    /// Stop after this many minutes without speech; 0 never stops.
    pub auto_stop_minutes: u32,
    /// Global start/stop shortcut (Tauri accelerator syntax); empty disables it.
    pub shortcut: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "zh-CN".into(),
            mic_device: None,
            record_system: true,
            auto_stop_minutes: 15,
            shortcut: "CmdOrCtrl+Shift+Alt+R".into(),
        }
    }
}

impl Settings {
    /// Settings from the library, defaults for anything missing or unreadable.
    pub fn load(store: &Store) -> Self {
        store
            .setting(KEY)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<Self>(&json).ok())
            .map(Self::sanitized)
            .unwrap_or_default()
    }

    /// Reject values the UI should never send.
    pub fn validate(&self) -> Result<(), CommandError> {
        if !LANGUAGES.contains(&self.language.as_str()) {
            return Err(CommandError::invalid("unsupported language"));
        }
        if self.auto_stop_minutes > MAX_AUTO_STOP_MINUTES {
            return Err(CommandError::invalid("auto-stop is too long"));
        }
        if self
            .mic_device
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 512)
        {
            return Err(CommandError::invalid("bad microphone id"));
        }
        if self.shortcut.len() > 64 {
            return Err(CommandError::invalid("shortcut is too long"));
        }
        if !self.shortcut.is_empty() && Shortcut::from_str(&self.shortcut).is_err() {
            return Err(CommandError::new("bad-shortcut", self.shortcut.clone()));
        }
        Ok(())
    }

    pub fn auto_stop(&self) -> Option<Duration> {
        (self.auto_stop_minutes > 0)
            .then(|| Duration::from_secs(60 * u64::from(self.auto_stop_minutes)))
    }

    /// Defaults for fields that would not validate (a hand-edited or older library).
    fn sanitized(mut self) -> Self {
        let defaults = Self::default();
        if !LANGUAGES.contains(&self.language.as_str()) {
            self.language = defaults.language;
        }
        if self.auto_stop_minutes > MAX_AUTO_STOP_MINUTES {
            self.auto_stop_minutes = defaults.auto_stop_minutes;
        }
        if self
            .mic_device
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 512)
        {
            self.mic_device = None;
        }
        if self.shortcut.len() > 64
            || (!self.shortcut.is_empty() && Shortcut::from_str(&self.shortcut).is_err())
        {
            self.shortcut = defaults.shortcut;
        }
        self
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("settings serialize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_validate_and_partial_json_keeps_defaults() {
        assert!(Settings::default().validate().is_ok());
        let parsed: Settings = serde_json::from_str(r#"{"language":"en"}"#).unwrap();
        assert_eq!(parsed.language, "en");
        assert!(parsed.record_system);
        assert_eq!(parsed.auto_stop(), Some(Duration::from_secs(15 * 60)));
    }

    #[test]
    fn invalid_values_are_rejected_and_sanitized() {
        let bad = Settings {
            language: "fr".into(),
            auto_stop_minutes: 10_000,
            shortcut: "Hyper+Banana".into(),
            mic_device: Some(String::new()),
            ..Settings::default()
        };
        assert_eq!(bad.validate().unwrap_err().code, "invalid");
        let clean = bad.sanitized();
        assert_eq!(clean, Settings::default());
        let no_shortcut = Settings {
            shortcut: String::new(),
            auto_stop_minutes: 0,
            ..Settings::default()
        };
        assert!(no_shortcut.validate().is_ok());
        assert_eq!(no_shortcut.auto_stop(), None);
        let shortcut = Settings {
            shortcut: "Ctrl+Nope".into(),
            ..Settings::default()
        };
        assert_eq!(shortcut.validate().unwrap_err().code, "bad-shortcut");
    }
}
