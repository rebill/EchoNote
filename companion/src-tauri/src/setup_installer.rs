use crate::logs::{append_log_line, LogStore};
use crate::offline_bundle::{self, VerifiedOfflineBundle};
use crate::path_resolver;
use crate::process::ProcessManagerState;
use crate::settings::{CompanionSettings, ModelPreset, SettingsStore};
use crate::setup_detector;
use crate::setup_types::{SetupDetection, SetupStatus, SetupStepId};
use std::ffi::OsString;
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
    let verified_bundle = resolve_verified_bundle(&detection)
        .map_err(|message| setup_error(SetupStepId::OfflineBundle, message))?;
    let python = resolve_or_prepare_python(&log_store, &detection, &service_dir)
        .map_err(|message| setup_error(SetupStepId::Python, message))?;
    if let Some(bundle) = verified_bundle.as_ref() {
        validate_bundle_python(bundle, &python)
            .map_err(|message| setup_error(SetupStepId::Python, message))?;
    }
    install_dependencies_if_needed(
        &log_store,
        &detection,
        verified_bundle.as_ref(),
        &python,
        &service_dir,
    )
    .map_err(|message| setup_error(SetupStepId::Dependencies, message))?;

    let mut settings = detection.settings.clone();
    install_models_if_needed(
        &log_store,
        &detection,
        verified_bundle.as_ref(),
        &mut settings,
    )
    .map_err(|message| setup_error(SetupStepId::Models, message))?;
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

fn resolve_verified_bundle(
    detection: &SetupDetection,
) -> Result<Option<VerifiedOfflineBundle>, String> {
    if detection.dependencies_ready && detection.models_ready {
        return Ok(None);
    }
    let root = detection
        .offline_bundle_path
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| {
            path_resolver::resolve_existing_directory(&detection.settings.offline_bundle_path)
        })
        .ok_or_else(|| {
            format!(
                "Offline bundle was not found at {}.",
                detection.settings.offline_bundle_path
            )
        })?;
    let preset = if detection.settings.model_preset == ModelPreset::Custom {
        "qwen3-0.6b-4bit"
    } else {
        detection.settings.selected_preset()
    };
    offline_bundle::verify_bundle(&root, preset, detection.settings.diarization_enabled).map(Some)
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
    let runtime_dir = path_resolver::expand_tilde(&detection.settings.runtime_path);
    let venv_dir = runtime_dir.join(".venv");
    let venv_python = venv_dir.join("bin/python");
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
    std::fs::create_dir_all(&runtime_dir)
        .map_err(|error| format!("Failed to create managed runtime directory: {error}"))?;
    let args = vec![
        OsString::from("-m"),
        OsString::from("venv"),
        venv_dir.as_os_str().to_owned(),
    ];
    run_logged(
        log_store,
        &system_python,
        &args,
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
    bundle: Option<&VerifiedOfflineBundle>,
    python: &Path,
    service_dir: &Path,
) -> Result<(), String> {
    if detection.dependencies_ready {
        log_setup(log_store, "Offline ASR dependencies are already installed.");
        return Ok(());
    }

    let bundle = bundle.ok_or_else(|| {
        "Offline bundle is required because ASR dependencies are not installed.".to_string()
    })?;
    log_setup(
        log_store,
        format!(
            "Installing dependencies without network access from {}.",
            bundle.wheels_dir.display()
        ),
    );
    let args = offline_pip_install_args(bundle);
    run_logged(
        log_store,
        python,
        &args,
        service_dir,
        "install dependencies from offline wheelhouse",
    )?;
    let python_path = python.to_string_lossy().into_owned();
    if !setup_detector::probe_dependencies(
        Some(&python_path),
        Some(service_dir),
        detection.settings.backend,
    ) {
        return Err(
            "Offline dependency installation completed, but ASR imports still fail.".to_string(),
        );
    }
    if detection.settings.diarization_enabled
        && !Command::new(python)
            .current_dir(service_dir)
            .arg("-c")
            .arg(setup_detector::DIARIZATION_DEPENDENCY_PROBE)
            .status()
            .is_ok_and(|status| status.success())
    {
        return Err(
            "Offline dependency installation completed, but pyannote.audio is unavailable."
                .to_string(),
        );
    }
    Ok(())
}

fn offline_pip_install_args(bundle: &VerifiedOfflineBundle) -> Vec<OsString> {
    vec![
        "-m".into(),
        "pip".into(),
        "install".into(),
        "--no-index".into(),
        "--disable-pip-version-check".into(),
        "--no-cache-dir".into(),
        "--find-links".into(),
        bundle.wheels_dir.as_os_str().to_owned(),
        "-r".into(),
        bundle.requirements_file.as_os_str().to_owned(),
    ]
}

fn install_models_if_needed(
    log_store: &LogStore,
    detection: &SetupDetection,
    bundle: Option<&VerifiedOfflineBundle>,
    settings: &mut CompanionSettings,
) -> Result<(), String> {
    if detection.models_ready {
        log_setup(log_store, "Required offline models are already installed.");
        return Ok(());
    }
    let bundle = bundle.ok_or_else(|| {
        "Offline bundle is required because local model files are missing.".to_string()
    })?;
    let target_root = path_resolver::expand_tilde(&settings.models_path);
    let installed = offline_bundle::install_models(bundle, &target_root)?;
    settings.models_path = target_root
        .canonicalize()
        .unwrap_or(target_root)
        .to_string_lossy()
        .into_owned();
    settings.asr_model_path = installed
        .asr_model_path
        .canonicalize()
        .unwrap_or(installed.asr_model_path)
        .to_string_lossy()
        .into_owned();
    settings.diarization_model_path = installed
        .diarization_model_path
        .map(|path| {
            path.canonicalize()
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_default();
    log_setup(
        log_store,
        format!(
            "Activated verified offline models at {}.",
            settings.models_path
        ),
    );
    Ok(())
}

fn validate_bundle_python(bundle: &VerifiedOfflineBundle, python: &Path) -> Result<(), String> {
    let output = Command::new(python)
        .arg("--version")
        .output()
        .map_err(|error| format!("Failed to inspect Python for offline bundle: {error}"))?;
    let raw = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).trim().to_string()
    } else {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };
    let actual = setup_detector::parse_python_version(&raw)
        .ok_or_else(|| format!("Could not parse Python version: {raw}"))?;
    let required = setup_detector::parse_python_version(&bundle.manifest.python_version)
        .ok_or_else(|| {
            format!(
                "Offline bundle contains an invalid Python version: {}",
                bundle.manifest.python_version
            )
        })?;
    if actual.0 != required.0 || actual.1 != required.1 {
        return Err(format!(
            "Offline bundle requires Python {}.{}, but {} reports {}. Rebuild the bundle for this Python minor version.",
            required.0,
            required.1,
            python.display(),
            raw
        ));
    }
    Ok(())
}

fn run_logged(
    log_store: &LogStore,
    command: &Path,
    args: &[OsString],
    cwd: &Path,
    label: &str,
) -> Result<(), String> {
    let command_line = format!(
        "{} {}",
        command.display(),
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    );
    log_setup(log_store, format!("Running {label}: {command_line}"));

    let output = Command::new(command)
        .current_dir(cwd)
        .env("PIP_NO_INDEX", "1")
        .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
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
    use super::offline_pip_install_args;
    use crate::offline_bundle::{OfflineBundleManifest, VerifiedOfflineBundle};
    use std::path::PathBuf;

    #[test]
    fn builds_strictly_offline_dependency_install_args() {
        let bundle = VerifiedOfflineBundle {
            root: PathBuf::from("/bundle"),
            manifest: OfflineBundleManifest {
                schema_version: 1,
                bundle_version: "test".to_string(),
                platform: "test".to_string(),
                python_version: "3.11".to_string(),
                wheels_path: "wheels".to_string(),
                requirements_path: "requirements-offline.txt".to_string(),
                models: Vec::new(),
                files: Vec::new(),
            },
            wheels_dir: PathBuf::from("/bundle/wheels"),
            requirements_file: PathBuf::from("/bundle/requirements-offline.txt"),
            asr_model_source: PathBuf::from("/bundle/models/asr"),
            diarization_model_source: None,
        };
        let args = offline_pip_install_args(&bundle)
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(args.contains(&"--no-index".to_string()));
        assert!(args.contains(&"--find-links".to_string()));
        assert!(!args.contains(&"--upgrade".to_string()));
        assert!(!args.contains(&"-e".to_string()));
        assert_eq!(
            args.last().map(String::as_str),
            Some("/bundle/requirements-offline.txt")
        );
    }
}
