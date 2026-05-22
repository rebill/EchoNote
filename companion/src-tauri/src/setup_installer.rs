use crate::logs::{append_log_line, LogStore};
use crate::process::ProcessManagerState;
use crate::settings::{Backend, CompanionSettings, SettingsStore};
use crate::setup_detector;
use crate::setup_types::{backend_dependency_extra, SetupDetection, SetupStatus, SetupStepId};
use std::path::{Path, PathBuf};
use std::process::Command;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub fn install_or_repair(
    app: &tauri::AppHandle,
    process_manager: &tauri::State<'_, ProcessManagerState>,
    detection: SetupDetection,
) -> Result<SetupDetection, SetupInstallError> {
    if matches!(detection.status, SetupStatus::Unsupported) {
        return Ok(detection);
    }

    let log_store =
        LogStore::new(app).map_err(|message| setup_error(SetupStepId::Runtime, message))?;
    log_setup(&log_store, "Starting setup or repair.");

    let service_dir = resolve_service_dir(&detection)
        .map_err(|message| setup_error(SetupStepId::Runtime, message))?;
    let python = resolve_or_prepare_python(&log_store, &detection, &service_dir)
        .map_err(|message| setup_error(SetupStepId::Python, message))?;
    install_dependencies_if_needed(&log_store, &detection, &python, &service_dir)
        .map_err(|message| setup_error(SetupStepId::Dependencies, message))?;

    let mut settings = detection.settings.clone();
    settings.python_path = python.to_string_lossy().into_owned();
    settings.asr_service_path = service_dir.to_string_lossy().into_owned();
    settings.setup_completed_at = Some(timestamp());
    settings.setup_version = Some(env!("CARGO_PKG_VERSION").to_string());
    SettingsStore::new(app)
        .and_then(|store| store.save(settings.clone()))
        .map_err(|message| setup_error(SetupStepId::Runtime, message))?;

    {
        let mut process_manager = process_manager.lock().map_err(|_| {
            setup_error(
                SetupStepId::Service,
                "Process manager state is unavailable.",
            )
        })?;
        process_manager.set_log_paths(log_store.companion_log_path(), log_store.asr_log_path());
        process_manager.start(&settings);
    }

    let runtime = {
        let mut process_manager = process_manager.lock().map_err(|_| {
            setup_error(
                SetupStepId::Service,
                "Process manager state is unavailable.",
            )
        })?;
        process_manager.snapshot(&settings)
    };
    Ok(setup_detector::detect(settings, &runtime))
}

#[derive(Debug, Clone)]
pub struct SetupInstallError {
    pub step_id: SetupStepId,
    pub message: String,
}

fn setup_error(step_id: SetupStepId, message: impl Into<String>) -> SetupInstallError {
    SetupInstallError {
        step_id,
        message: message.into(),
    }
}

pub fn reset_settings(app: &tauri::AppHandle) -> Result<CompanionSettings, String> {
    let settings = CompanionSettings::default();
    SettingsStore::new(app)?.save(settings.clone())?;
    Ok(settings)
}

fn resolve_service_dir(detection: &SetupDetection) -> Result<PathBuf, String> {
    detection
        .asr_service_path
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| "EchoNote could not find the ASR service source.".to_string())
}

fn resolve_or_prepare_python(
    log_store: &LogStore,
    detection: &SetupDetection,
    service_dir: &Path,
) -> Result<PathBuf, String> {
    let venv_python = service_dir.join(".venv/bin/python");
    if venv_python.is_file() {
        log_setup(
            log_store,
            format!(
                "Using existing ASR virtual environment at {}.",
                venv_python.display()
            ),
        );
        return Ok(venv_python);
    }

    let Some(system_python) = detection
        .python_candidates
        .iter()
        .find(|candidate| candidate.valid && PathBuf::from(&candidate.path) != venv_python)
        .map(|candidate| PathBuf::from(&candidate.path))
        .or_else(|| detection.python_path.as_ref().map(PathBuf::from))
    else {
        return Err("EchoNote could not find Python 3.11 or newer.".to_string());
    };

    log_setup(
        log_store,
        format!(
            "Creating ASR virtual environment with {}.",
            system_python.display()
        ),
    );
    run_logged(
        log_store,
        &system_python,
        &["-m", "venv", ".venv"],
        service_dir,
        "create virtual environment",
    )?;

    if venv_python.is_file() {
        Ok(venv_python)
    } else {
        Err(format!(
            "Virtual environment was created but Python was not found at {}.",
            venv_python.display()
        ))
    }
}

fn install_dependencies_if_needed(
    log_store: &LogStore,
    detection: &SetupDetection,
    python: &Path,
    service_dir: &Path,
) -> Result<(), String> {
    let python_path = python.to_string_lossy().into_owned();
    if setup_detector::probe_dependencies(
        Some(&python_path),
        Some(service_dir),
        detection.settings.backend,
    ) {
        log_setup(log_store, "ASR dependencies already look ready.");
        return Ok(());
    }

    log_setup(log_store, "Installing ASR dependencies.");
    run_logged(
        log_store,
        python,
        &["-m", "pip", "install", "--upgrade", "pip"],
        service_dir,
        "upgrade pip",
    )?;

    let extra = backend_dependency_extra(detection.settings.backend);
    run_logged(
        log_store,
        python,
        &install_command_args(extra),
        service_dir,
        if detection.settings.backend == Backend::MlxAudio {
            "install ASR service with MLX dependencies"
        } else {
            "install ASR service"
        },
    )
}

fn install_command_args(extra: &'static str) -> [&'static str; 5] {
    ["-m", "pip", "install", "-e", extra]
}

fn run_logged(
    log_store: &LogStore,
    command: &Path,
    args: &[&str],
    cwd: &Path,
    label: &str,
) -> Result<(), String> {
    let command_line = format!(
        "{} {}",
        command.display(),
        args.iter()
            .map(|arg| arg.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    );
    log_setup(log_store, format!("Running {label}: {command_line}"));

    let output = Command::new(command)
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(|error| format!("Failed to run {label}: {error}"))?;

    if !output.stdout.is_empty() {
        log_setup(
            log_store,
            format!(
                "{label} stdout: {}",
                String::from_utf8_lossy(&output.stdout).trim()
            ),
        );
    }
    if !output.stderr.is_empty() {
        log_setup(
            log_store,
            format!(
                "{label} stderr: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        );
    }

    if output.status.success() {
        Ok(())
    } else {
        Err(format!("{label} failed with status {}", output.status))
    }
}

fn log_setup(log_store: &LogStore, message: impl AsRef<str>) {
    let _ = log_store.ensure_dir();
    append_log_line(&log_store.companion_log_path(), message.as_ref());
}

fn timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::install_command_args;
    use crate::settings::Backend;
    use crate::setup_types::backend_dependency_extra;

    #[test]
    fn builds_dependency_install_args_without_shell() {
        assert_eq!(
            install_command_args(backend_dependency_extra(Backend::Fake)),
            ["-m", "pip", "install", "-e", "."]
        );
        assert_eq!(
            install_command_args(backend_dependency_extra(Backend::MlxAudio)),
            ["-m", "pip", "install", "-e", ".[mlx]"]
        );
    }
}
