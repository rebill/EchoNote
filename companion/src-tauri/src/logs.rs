use crate::process::ProcessManagerState;
use crate::settings::{CompanionSettings, SettingsStore};
use crate::setup;
use crate::state::RuntimeState;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::Manager;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const LOG_LINE_LIMIT: usize = 2_000;
const RECENT_LOG_LIMIT: usize = 80;

#[derive(Debug, Clone)]
pub struct LogStore {
    logs_dir: PathBuf,
    companion_log_path: PathBuf,
    asr_log_path: PathBuf,
}

impl LogStore {
    pub fn new(app: &tauri::AppHandle) -> Result<Self, String> {
        let home_dir = app
            .path()
            .home_dir()
            .map_err(|error| format!("Failed to resolve home directory: {error}"))?;
        let logs_dir = home_dir.join("Library").join("Logs").join("EchoNote");

        Ok(Self {
            companion_log_path: logs_dir.join("companion.log"),
            asr_log_path: logs_dir.join("asr-service.log"),
            logs_dir,
        })
    }

    pub fn logs_dir_string(&self) -> String {
        self.logs_dir.to_string_lossy().into_owned()
    }

    pub fn companion_log_path(&self) -> PathBuf {
        self.companion_log_path.clone()
    }

    pub fn asr_log_path(&self) -> PathBuf {
        self.asr_log_path.clone()
    }

    pub fn ensure_dir(&self) -> Result<(), String> {
        fs::create_dir_all(&self.logs_dir)
            .map_err(|error| format!("Failed to create logs directory: {error}"))
    }

    pub fn open_logs_folder(&self) -> Result<(), String> {
        self.ensure_dir()?;
        let opener = platform_opener();
        Command::new(opener)
            .arg(&self.logs_dir)
            .spawn()
            .map_err(|error| format!("Failed to open logs folder with {opener}: {error}"))?;
        Ok(())
    }

    pub fn recent_logs(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.extend(read_recent_lines(
            &self.companion_log_path,
            RECENT_LOG_LIMIT / 2,
        ));
        lines.extend(read_recent_lines(&self.asr_log_path, RECENT_LOG_LIMIT / 2));
        if lines.len() > RECENT_LOG_LIMIT {
            lines.drain(0..(lines.len() - RECENT_LOG_LIMIT));
        }
        lines
    }
}

pub fn append_log_line(path: &Path, message: &str) {
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }

    let timestamp = timestamp();
    let line = sanitize_log_line(message);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{timestamp} {line}");
    }
}

pub fn sanitize_log_line(value: &str) -> String {
    truncate_chars(&redact_secrets(value), LOG_LINE_LIMIT)
}

pub fn diagnostic_report(
    app: &tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<String, String> {
    let settings_response = SettingsStore::new(app)?.load_or_default()?;
    let log_store = LogStore::new(app)?;
    let runtime = process_manager
        .lock()
        .map_err(|_| "Process manager state is unavailable.".to_string())?
        .snapshot(&settings_response.settings);

    Ok(build_diagnostic_report(
        app,
        &settings_response.settings,
        &settings_response.settings_path,
        &log_store,
        &runtime,
    ))
}

fn build_diagnostic_report(
    app: &tauri::AppHandle,
    settings: &CompanionSettings,
    settings_path: &str,
    log_store: &LogStore,
    runtime: &RuntimeState,
) -> String {
    let recent_logs = log_store.recent_logs();
    let mut report = String::new();
    report.push_str("# EchoNote Diagnostic Report\n\n");
    report.push_str(&format!("- Generated at: {}\n", timestamp()));
    report.push_str(&format!(
        "- Companion version: {}\n",
        env!("CARGO_PKG_VERSION")
    ));
    report.push_str(&format!(
        "- Platform: {} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    report.push_str(&format!("- Service status: {:?}\n", runtime.service_status));
    report.push_str(&format!("- Model status: {:?}\n", runtime.model_status));
    report.push_str(&format!("- Backend: {:?}\n", runtime.backend));
    report.push_str(&format!("- Model ID: {}\n", runtime.resolved_model_id));
    report.push_str(&format!(
        "- Base URL: {}\n",
        runtime.base_url.as_deref().unwrap_or("Unavailable")
    ));
    report.push_str(&format!(
        "- PID: {}\n",
        runtime
            .pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "None".to_string())
    ));
    report.push_str(&format!(
        "- Last exit code: {}\n",
        runtime
            .last_exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "None".to_string())
    ));
    report.push_str(&format!(
        "- Last error: {}\n",
        runtime.last_error.as_deref().unwrap_or("None")
    ));
    report.push_str(&format!("- Python path: {}\n", settings.python_path));
    report.push_str(&format!(
        "- ASR service path: {}\n",
        settings.asr_service_path
    ));
    report.push_str(&format!("- Settings path: {settings_path}\n"));
    report.push_str(&format!(
        "- Logs path: {}\n",
        log_store.logs_dir.to_string_lossy()
    ));
    report.push_str(&format!(
        "- ASR log path: {}\n",
        log_store.asr_log_path.to_string_lossy()
    ));
    report.push_str("\n## Setup\n\n");
    for line in setup::setup_diagnostic_lines(app, settings, runtime) {
        report.push_str(&line);
        report.push('\n');
    }
    report.push_str("\n## Recent Logs\n\n");

    if recent_logs.is_empty() {
        report.push_str("_No log lines available._\n");
    } else {
        for line in recent_logs {
            report.push_str("- ");
            report.push_str(&sanitize_log_line(&line));
            report.push('\n');
        }
    }

    redact_secrets(&report)
}

fn read_recent_lines(path: &Path, max_lines: usize) -> Vec<String> {
    let Ok(file) = OpenOptions::new().read(true).open(path) else {
        return Vec::new();
    };
    let reader = BufReader::new(file);
    let mut lines = reader
        .lines()
        .map_while(Result::ok)
        .map(|line| sanitize_log_line(&line))
        .collect::<Vec<_>>();
    if lines.len() > max_lines {
        lines.drain(0..(lines.len() - max_lines));
    }
    lines
}

fn timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn platform_opener() -> &'static str {
    if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index >= max_chars {
            output.push_str("...[truncated]");
            return output;
        }
        output.push(ch);
    }
    output
}

fn redact_secrets(value: &str) -> String {
    let without_bearer = redact_bearer_tokens(value);
    let without_keys = redact_sk_keys(&without_bearer);
    redact_env_secret_values(&without_keys)
}

fn redact_bearer_tokens(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let marker = "authorization: bearer ";
    let mut output = String::new();
    let mut cursor = 0;

    while let Some(relative_index) = lower[cursor..].find(marker) {
        let marker_start = cursor + relative_index;
        let token_start = marker_start + marker.len();
        output.push_str(&value[cursor..token_start]);

        let token_end = value[token_start..]
            .find(char::is_whitespace)
            .map(|offset| token_start + offset)
            .unwrap_or(value.len());
        output.push_str("[REDACTED]");
        cursor = token_end;
    }

    output.push_str(&value[cursor..]);
    output
}

fn redact_sk_keys(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.char_indices().peekable();

    while let Some((index, ch)) = chars.next() {
        if ch == 's' && value[index..].starts_with("sk-") {
            output.push_str("sk-[REDACTED]");
            while let Some((_, next_ch)) = chars.peek() {
                if next_ch.is_ascii_alphanumeric() || *next_ch == '_' || *next_ch == '-' {
                    let _ = chars.next();
                } else {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }

    output
}

fn redact_env_secret_values(value: &str) -> String {
    const SECRET_MARKERS: [&str; 4] = ["API_KEY=", "TOKEN=", "SECRET=", "PASSWORD="];
    let mut line = value.to_string();
    for marker in SECRET_MARKERS {
        if let Some(start) = line.find(marker) {
            let value_start = start + marker.len();
            let value_end = line[value_start..]
                .find(char::is_whitespace)
                .map(|offset| value_start + offset)
                .unwrap_or(line.len());
            line.replace_range(value_start..value_end, "[REDACTED]");
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::sanitize_log_line;

    #[test]
    fn redacts_secrets_and_truncates_long_lines() {
        let sanitized = sanitize_log_line(
            "Authorization: Bearer abc123 sk-testsecret OPENAI_API_KEY=secret-value",
        );
        assert!(sanitized.contains("Authorization: Bearer [REDACTED]"));
        assert!(sanitized.contains("sk-[REDACTED]"));
        assert!(sanitized.contains("OPENAI_API_KEY=[REDACTED]"));

        let long = "a".repeat(2_100);
        assert!(sanitize_log_line(&long).ends_with("...[truncated]"));
    }
}
