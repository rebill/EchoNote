use crate::offline_bundle;
use crate::path_resolver;
use crate::process;
use crate::settings::{Backend, CompanionSettings, ModelPreset};
use crate::setup_types::{
    PythonCandidate, SetupDetection, SetupPrimaryAction, SetupStatus, SetupStep, SetupStepId,
    SetupStepStatus,
};
use crate::state::{RuntimeState, ServiceStatus};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;

const MIN_PYTHON_MAJOR: u32 = 3;
const MIN_PYTHON_MINOR: u32 = 11;
pub(crate) const DIARIZATION_DEPENDENCY_PROBE: &str = "import importlib.metadata as m; version = m.version('pyannote.audio'); raise SystemExit(0 if int(version.split('.', 1)[0]) == 4 else 1)";

pub fn detect(settings: CompanionSettings, runtime: &RuntimeState) -> SetupDetection {
    let system = detect_system_step();
    let asr_service_path = resolve_asr_service_path(&settings.asr_service_path);
    let python_candidates = python_candidates(&settings, asr_service_path.as_deref());
    let python_path = python_candidates
        .iter()
        .find(|candidate| candidate.valid)
        .map(|candidate| candidate.path.clone());

    let runtime_step = runtime_step(&asr_service_path, &settings.asr_service_path);
    let python_step = python_step(&python_candidates);
    let base_dependencies_ready = probe_dependencies(
        python_path.as_deref(),
        asr_service_path.as_deref(),
        settings.backend,
    );
    let diarization_dependency_ready = probe_python_import(
        python_path.as_deref(),
        asr_service_path.as_deref(),
        DIARIZATION_DEPENDENCY_PROBE,
    );
    let dependencies_ready =
        base_dependencies_ready && (!settings.diarization_enabled || diarization_dependency_ready);
    let dependencies_step = dependencies_step(
        base_dependencies_ready,
        python_path.as_deref(),
        settings.diarization_enabled,
        diarization_dependency_ready,
    );
    let models_ready = installed_models_ready(&settings);
    let models_step = models_step(&settings, models_ready);
    let bundle_required = !dependencies_ready || !models_ready;
    let offline_bundle_path =
        path_resolver::resolve_existing_directory(&settings.offline_bundle_path);
    let selected_python_version = python_candidates
        .iter()
        .find(|candidate| candidate.valid)
        .and_then(|candidate| candidate.version.as_deref());
    let bundle_probe = offline_bundle_path.as_deref().map(|path| {
        let bundle = offline_bundle::probe_bundle(
            path,
            bundle_preset(&settings),
            settings.diarization_enabled,
        )?;
        if let Some(actual_python) = selected_python_version {
            validate_bundle_python_version(&bundle.manifest.python_version, actual_python)?;
        }
        Ok(bundle)
    });
    let offline_bundle_ready = bundle_probe.as_ref().is_some_and(Result::is_ok);
    let offline_bundle_step = offline_bundle_step(
        &settings.offline_bundle_path,
        offline_bundle_path.as_deref(),
        bundle_probe.as_ref(),
        bundle_required,
    );
    let existing_service_healthy =
        process::request_existing_asr_health(settings.preferred_port).is_ok();
    let port_available = existing_service_healthy || port_is_available(settings.preferred_port);
    let port_step = port_step(
        settings.preferred_port,
        port_available,
        existing_service_healthy,
    );
    let service_step = service_step(runtime);
    let model_step = model_step(runtime);
    let obsidian_step = obsidian_step(runtime);

    let steps = vec![
        system,
        python_step,
        runtime_step,
        offline_bundle_step,
        dependencies_step,
        models_step,
        port_step,
        service_step,
        model_step,
        obsidian_step,
    ];

    let (status, primary_action, message) = derive_status(&steps, runtime.service_status);

    SetupDetection {
        status,
        steps,
        settings,
        primary_action,
        message,
        python_path,
        asr_service_path: asr_service_path.map(|path| path.to_string_lossy().into_owned()),
        python_candidates,
        offline_bundle_path: offline_bundle_path.map(|path| path.to_string_lossy().into_owned()),
        offline_bundle_ready,
        dependencies_ready,
        models_ready,
        port_available,
        existing_service_healthy,
    }
}

pub fn resolve_asr_service_path(value: &str) -> Option<PathBuf> {
    path_resolver::resolve_asr_service_dir(value)
}

pub fn parse_python_version(raw: &str) -> Option<(u32, u32, u32)> {
    let version = raw
        .split_whitespace()
        .find(|part| part.chars().next().is_some_and(|ch| ch.is_ascii_digit()))?;
    let mut parts = version.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    let patch = parts
        .next()
        .and_then(|value| {
            value
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>()
                .parse::<u32>()
                .ok()
        })
        .unwrap_or(0);
    Some((major, minor, patch))
}

pub fn python_version_is_supported(raw: &str) -> bool {
    parse_python_version(raw).is_some_and(|(major, minor, _)| {
        major > MIN_PYTHON_MAJOR || (major == MIN_PYTHON_MAJOR && minor >= MIN_PYTHON_MINOR)
    })
}

fn validate_bundle_python_version(required: &str, actual: &str) -> Result<(), String> {
    let required = parse_python_version(required)
        .ok_or_else(|| format!("Offline bundle contains an invalid Python version: {required}"))?;
    let actual_version = parse_python_version(actual)
        .ok_or_else(|| format!("Could not parse selected Python version: {actual}"))?;
    if required.0 == actual_version.0 && required.1 == actual_version.1 {
        Ok(())
    } else {
        Err(format!(
            "Offline bundle requires Python {}.{}, but the selected runtime is {actual}.",
            required.0, required.1
        ))
    }
}

fn detect_system_step() -> SetupStep {
    if std::env::consts::OS != "macos" {
        return SetupStep::new(
            SetupStepId::System,
            "Check System",
            SetupStepStatus::Failed,
            "EchoNote currently supports macOS only.",
            false,
        )
        .with_detail(format!(
            "Detected platform: {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
    }

    SetupStep::new(
        SetupStepId::System,
        "Check System",
        SetupStepStatus::Passed,
        "This Mac can run EchoNote setup.",
        false,
    )
}

fn python_candidates(
    settings: &CompanionSettings,
    service_dir: Option<&Path>,
) -> Vec<PythonCandidate> {
    let mut candidates = Vec::<String>::new();
    let configured_python = settings.python_path.trim();
    if !is_generic_python_command(configured_python) {
        push_unique(
            &mut candidates,
            path_resolver::expand_tilde(configured_python).to_string_lossy(),
        );
    }
    push_unique(
        &mut candidates,
        path_resolver::expand_tilde(&settings.runtime_path)
            .join(".venv/bin/python")
            .to_string_lossy(),
    );
    if let Some(service_dir) = service_dir {
        push_unique(
            &mut candidates,
            service_dir.join(".venv/bin/python").to_string_lossy(),
        );
    }
    for candidate in macos_python_candidates() {
        push_unique(&mut candidates, candidate.to_string_lossy());
    }
    if is_generic_python_command(configured_python) {
        push_unique(&mut candidates, configured_python);
    }
    push_unique(&mut candidates, "python3");
    push_unique(&mut candidates, "python");

    candidates
        .into_iter()
        .map(|path| inspect_python_candidate(path))
        .collect()
}

fn is_generic_python_command(value: &str) -> bool {
    matches!(value, "python" | "python3")
}

fn macos_python_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home_dir) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home_dir).join(".pyenv/shims/python3"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/python3"));
    candidates.push(PathBuf::from("/usr/local/bin/python3"));
    candidates.push(PathBuf::from("/usr/bin/python3"));
    candidates
}

fn inspect_python_candidate(path: String) -> PythonCandidate {
    let output = Command::new(&path).arg("--version").output();
    match output {
        Ok(output) if output.status.success() => {
            let raw = if output.stdout.is_empty() {
                String::from_utf8_lossy(&output.stderr).trim().to_string()
            } else {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            };
            let valid = python_version_is_supported(&raw);
            PythonCandidate {
                path,
                valid,
                version: Some(raw.clone()),
                error: (!valid).then(|| {
                    format!("Python version is below {MIN_PYTHON_MAJOR}.{MIN_PYTHON_MINOR}: {raw}")
                }),
            }
        }
        Ok(output) => PythonCandidate {
            path,
            valid: false,
            version: None,
            error: Some(format!(
                "python --version exited with status {}",
                output.status
            )),
        },
        Err(error) => PythonCandidate {
            path,
            valid: false,
            version: None,
            error: Some(error.to_string()),
        },
    }
}

fn runtime_step(path: &Option<PathBuf>, configured_path: &str) -> SetupStep {
    match path {
        Some(path) => SetupStep::new(
            SetupStepId::Runtime,
            "Prepare ASR Runtime",
            SetupStepStatus::Passed,
            "ASR service source is available.",
            true,
        )
        .with_detail(path.to_string_lossy()),
        None => SetupStep::new(
            SetupStepId::Runtime,
            "Prepare ASR Runtime",
            SetupStepStatus::Failed,
            "EchoNote could not find the ASR service source.",
            true,
        )
        .with_detail(format!("Configured path: {configured_path}")),
    }
}

fn python_step(candidates: &[PythonCandidate]) -> SetupStep {
    if let Some(candidate) = candidates.iter().find(|candidate| candidate.valid) {
        return SetupStep::new(
            SetupStepId::Python,
            "Find Python",
            SetupStepStatus::Passed,
            "Python 3.11 or newer is available.",
            true,
        )
        .with_detail(format!(
            "{} ({})",
            candidate.path,
            candidate.version.as_deref().unwrap_or("version unknown")
        ));
    }

    SetupStep::new(
        SetupStepId::Python,
        "Find Python",
        SetupStepStatus::Failed,
        "EchoNote could not find Python 3.11 or newer.",
        true,
    )
}

pub(crate) fn probe_dependencies(
    python_path: Option<&str>,
    service_dir: Option<&Path>,
    backend: crate::settings::Backend,
) -> bool {
    let (Some(python_path), Some(service_dir)) = (python_path, service_dir) else {
        return false;
    };

    let mut script = "import fastapi, uvicorn, echonote_asr".to_string();
    if backend == crate::settings::Backend::MlxAudio {
        script.push_str("\nimport mlx_audio");
    }

    Command::new(python_path)
        .current_dir(service_dir)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg("-c")
        .arg(script)
        .status()
        .is_ok_and(|status| status.success())
}

fn dependencies_step(
    ready: bool,
    python_path: Option<&str>,
    diarization_enabled: bool,
    diarization_dependency_ready: bool,
) -> SetupStep {
    if ready && (!diarization_enabled || diarization_dependency_ready) {
        let mut step = SetupStep::new(
            SetupStepId::Dependencies,
            "Install Dependencies",
            SetupStepStatus::Passed,
            "ASR dependencies are installed.",
            true,
        );
        if diarization_enabled {
            let detail = if diarization_dependency_ready {
                "Speaker diarization dependency is installed for offline use."
            } else {
                "Speaker diarization dependency is missing and must be installed from the offline bundle."
            };
            step = step.with_detail(detail);
        }
        return step;
    }

    if ready && diarization_enabled {
        return SetupStep::new(
            SetupStepId::Dependencies,
            "Install Dependencies",
            SetupStepStatus::Warning,
            "Speaker diarization dependencies need to be installed from the offline bundle.",
            true,
        );
    }

    let status = if python_path.is_some() {
        SetupStepStatus::Warning
    } else {
        SetupStepStatus::Skipped
    };
    SetupStep::new(
        SetupStepId::Dependencies,
        "Install Dependencies",
        status,
        "ASR dependencies need to be installed or repaired.",
        true,
    )
}

fn offline_bundle_step(
    configured_path: &str,
    resolved_path: Option<&Path>,
    probe: Option<&Result<offline_bundle::VerifiedOfflineBundle, String>>,
    required: bool,
) -> SetupStep {
    match probe {
        Some(Ok(bundle)) => SetupStep::new(
            SetupStepId::OfflineBundle,
            "Verify Offline Bundle",
            SetupStepStatus::Passed,
            "Offline installation bundle is available.",
            true,
        )
        .with_detail(format!(
            "{} (bundle {}, Python {})",
            bundle.root.display(),
            bundle.manifest.bundle_version,
            bundle.manifest.python_version
        )),
        Some(Err(error)) if required => SetupStep::new(
            SetupStepId::OfflineBundle,
            "Verify Offline Bundle",
            SetupStepStatus::Failed,
            "Offline bundle is invalid and setup cannot continue.",
            true,
        )
        .with_detail(error),
        Some(Err(error)) => SetupStep::new(
            SetupStepId::OfflineBundle,
            "Verify Offline Bundle",
            SetupStepStatus::Warning,
            "Installed runtime is usable, but the configured repair bundle is invalid.",
            true,
        )
        .with_detail(error),
        None if required => SetupStep::new(
            SetupStepId::OfflineBundle,
            "Verify Offline Bundle",
            SetupStepStatus::Failed,
            "Offline bundle is required to install or repair EchoNote.",
            true,
        )
        .with_detail(format!("Configured path: {configured_path}")),
        None => SetupStep::new(
            SetupStepId::OfflineBundle,
            "Verify Offline Bundle",
            SetupStepStatus::Warning,
            "Offline bundle is not mounted; the installed runtime can still run.",
            true,
        )
        .with_detail(
            resolved_path
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("Configured path: {configured_path}")),
        ),
    }
}

fn installed_models_ready(settings: &CompanionSettings) -> bool {
    if settings.backend == Backend::Fake {
        return true;
    }
    let asr_path = path_resolver::expand_tilde(&settings.resolved_model_id());
    if !asr_path.is_dir() || !asr_path.join("config.json").is_file() {
        return false;
    }
    if settings.diarization_enabled {
        let diarization = path_resolver::expand_tilde(&settings.diarization_model_path);
        if !diarization.is_dir() || !diarization.join("config.yaml").is_file() {
            return false;
        }
    }
    true
}

fn models_step(settings: &CompanionSettings, ready: bool) -> SetupStep {
    if ready {
        return SetupStep::new(
            SetupStepId::Models,
            "Install Offline Models",
            SetupStepStatus::Passed,
            "Required local model files are installed.",
            true,
        )
        .with_detail(settings.resolved_model_id());
    }

    SetupStep::new(
        SetupStepId::Models,
        "Install Offline Models",
        SetupStepStatus::Warning,
        "Local ASR or diarization models need to be installed from the offline bundle.",
        true,
    )
}

fn bundle_preset(settings: &CompanionSettings) -> &'static str {
    if settings.model_preset == ModelPreset::Custom {
        "qwen3-0.6b-4bit"
    } else {
        settings.selected_preset()
    }
}

fn probe_python_import(
    python_path: Option<&str>,
    service_dir: Option<&Path>,
    script: &str,
) -> bool {
    let (Some(python_path), Some(service_dir)) = (python_path, service_dir) else {
        return false;
    };

    Command::new(python_path)
        .current_dir(service_dir)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg("-c")
        .arg(script)
        .status()
        .is_ok_and(|status| status.success())
}

fn port_step(port: u16, available: bool, existing_service_healthy: bool) -> SetupStep {
    if existing_service_healthy {
        return SetupStep::new(
            SetupStepId::Port,
            "Check Port",
            SetupStepStatus::Passed,
            format!("Port {port} already has a healthy EchoNote ASR service."),
            true,
        );
    }

    if available {
        return SetupStep::new(
            SetupStepId::Port,
            "Check Port",
            SetupStepStatus::Passed,
            format!("Port {port} is available."),
            true,
        );
    }

    SetupStep::new(
        SetupStepId::Port,
        "Check Port",
        SetupStepStatus::Failed,
        format!("Port {port} is already in use."),
        true,
    )
}

fn service_step(runtime: &RuntimeState) -> SetupStep {
    match runtime.service_status {
        ServiceStatus::Running => SetupStep::new(
            SetupStepId::Service,
            "Start Service",
            SetupStepStatus::Passed,
            "Local transcription service is running.",
            true,
        ),
        ServiceStatus::Error => SetupStep::new(
            SetupStepId::Service,
            "Start Service",
            SetupStepStatus::Failed,
            "EchoNote could not start the ASR service.",
            true,
        )
        .with_detail(
            runtime
                .last_error
                .as_deref()
                .unwrap_or("Unknown service error."),
        ),
        _ => SetupStep::new(
            SetupStepId::Service,
            "Start Service",
            SetupStepStatus::Pending,
            "Local transcription service is not running yet.",
            true,
        ),
    }
}

fn model_step(runtime: &RuntimeState) -> SetupStep {
    use crate::state::ModelStatus;
    match runtime.model_status {
        ModelStatus::Ready => SetupStep::new(
            SetupStepId::Model,
            "Verify Model",
            SetupStepStatus::Passed,
            "ASR model is ready.",
            true,
        ),
        ModelStatus::Loading => SetupStep::new(
            SetupStepId::Model,
            "Verify Model",
            SetupStepStatus::Running,
            "ASR model is loading.",
            true,
        ),
        ModelStatus::Error => SetupStep::new(
            SetupStepId::Model,
            "Verify Model",
            SetupStepStatus::Failed,
            "EchoNote could not load the selected ASR model.",
            true,
        ),
        _ => SetupStep::new(
            SetupStepId::Model,
            "Verify Model",
            SetupStepStatus::Pending,
            "ASR model will be checked after service start.",
            true,
        ),
    }
}

fn obsidian_step(runtime: &RuntimeState) -> SetupStep {
    if runtime.service_status == ServiceStatus::Running {
        SetupStep::new(
            SetupStepId::Obsidian,
            "Connect Obsidian",
            SetupStepStatus::Passed,
            "Open Obsidian and check EchoNote Status.",
            true,
        )
    } else {
        SetupStep::new(
            SetupStepId::Obsidian,
            "Connect Obsidian",
            SetupStepStatus::Pending,
            "Obsidian can connect after the local service starts.",
            true,
        )
    }
}

fn derive_status(
    steps: &[SetupStep],
    service_status: ServiceStatus,
) -> (SetupStatus, SetupPrimaryAction, String) {
    if steps
        .iter()
        .any(|step| step.id == SetupStepId::System && step.status == SetupStepStatus::Failed)
    {
        return (
            SetupStatus::Unsupported,
            SetupPrimaryAction::None,
            "This Mac is not supported.".to_string(),
        );
    }

    if service_status == ServiceStatus::Running {
        return (
            SetupStatus::Running,
            SetupPrimaryAction::Stop,
            "Local transcription is running.".to_string(),
        );
    }

    let has_failed_required = steps.iter().any(|step| {
        matches!(
            step.id,
            SetupStepId::Python
                | SetupStepId::Runtime
                | SetupStepId::OfflineBundle
                | SetupStepId::Port
        ) && step.status == SetupStepStatus::Failed
    });
    if has_failed_required {
        return (
            SetupStatus::RepairRequired,
            SetupPrimaryAction::Repair,
            "EchoNote found an issue it can repair.".to_string(),
        );
    }

    let has_failed_runtime_verification = steps.iter().any(|step| {
        matches!(step.id, SetupStepId::Service | SetupStepId::Model)
            && step.status == SetupStepStatus::Failed
    });
    if has_failed_runtime_verification {
        return (
            SetupStatus::Error,
            SetupPrimaryAction::Retry,
            "EchoNote could not complete setup.".to_string(),
        );
    }

    let dependencies_ready = steps
        .iter()
        .any(|step| step.id == SetupStepId::Dependencies && step.status == SetupStepStatus::Passed);
    let models_ready = steps
        .iter()
        .any(|step| step.id == SetupStepId::Models && step.status == SetupStepStatus::Passed);
    if dependencies_ready && models_ready {
        return (
            SetupStatus::Ready,
            SetupPrimaryAction::Start,
            "EchoNote is ready.".to_string(),
        );
    }

    (
        SetupStatus::NotConfigured,
        SetupPrimaryAction::Setup,
        "Set up EchoNote to use local transcription.".to_string(),
    )
}

fn port_is_available(port: u16) -> bool {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpListener::bind(address).is_ok()
}

fn push_unique(values: &mut Vec<String>, value: impl AsRef<str>) {
    let value = value.as_ref().trim();
    if !value.is_empty() && !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        derive_status, parse_python_version, python_candidates, python_version_is_supported,
        resolve_asr_service_path, validate_bundle_python_version,
    };
    use crate::path_resolver;
    use crate::settings::CompanionSettings;
    use crate::setup_types::{
        SetupPrimaryAction, SetupStatus, SetupStep, SetupStepId, SetupStepStatus,
    };
    use crate::state::ServiceStatus;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_python_version_output() {
        assert_eq!(parse_python_version("Python 3.11.9"), Some((3, 11, 9)));
        assert_eq!(parse_python_version("Python 3.12.0rc1"), Some((3, 12, 0)));
        assert_eq!(parse_python_version("not python"), None);
    }

    #[test]
    fn accepts_only_supported_python_versions() {
        assert!(python_version_is_supported("Python 3.11.0"));
        assert!(python_version_is_supported("Python 3.12.1"));
        assert!(!python_version_is_supported("Python 3.10.13"));
    }

    #[test]
    fn offline_bundle_requires_an_exact_python_minor() {
        assert!(validate_bundle_python_version("3.11", "Python 3.11.9").is_ok());
        assert!(validate_bundle_python_version("3.11", "Python 3.12.1").is_err());
    }

    #[test]
    fn validates_asr_service_path_shape() {
        let path = temp_service_dir();
        assert_eq!(
            resolve_asr_service_path(&path.to_string_lossy()),
            Some(path.canonicalize().expect("canonical temp path"))
        );
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn orders_python_candidates_from_explicit_to_fallbacks() {
        let path = temp_service_dir();
        let settings = CompanionSettings {
            python_path: "/tmp/echonote-custom-python".to_string(),
            ..CompanionSettings::default()
        };

        let candidates = python_candidates(&settings, Some(&path));
        let paths = candidates
            .iter()
            .map(|candidate| candidate.path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(paths[0], "/tmp/echonote-custom-python");
        let managed_python = path_resolver::expand_tilde(&settings.runtime_path)
            .join(".venv/bin/python")
            .to_string_lossy()
            .to_string();
        assert_eq!(paths[1], managed_python);
        let venv_python = path.join(".venv/bin/python").to_string_lossy().to_string();
        assert_eq!(paths[2], venv_python);
        assert!(paths.contains(&"python3"));
        assert!(paths.contains(&"python"));

        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn generic_python_setting_does_not_hide_existing_service_venv() {
        let path = temp_service_dir();
        let settings = CompanionSettings {
            python_path: "python3".to_string(),
            ..CompanionSettings::default()
        };

        let candidates = python_candidates(&settings, Some(&path));
        let paths = candidates
            .iter()
            .map(|candidate| candidate.path.as_str())
            .collect::<Vec<_>>();
        let managed_python = path_resolver::expand_tilde(&settings.runtime_path)
            .join(".venv/bin/python")
            .to_string_lossy()
            .to_string();

        assert_eq!(paths[0], managed_python);
        let venv_python = path.join(".venv/bin/python").to_string_lossy().to_string();
        assert_eq!(paths[1], venv_python);
        assert!(paths.contains(&"python3"));

        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn keeps_python_candidate_failures_in_detection_output() {
        let candidate = python_candidates(
            &CompanionSettings {
                python_path: "/tmp/echonote-missing-python".to_string(),
                ..CompanionSettings::default()
            },
            None,
        )
        .into_iter()
        .find(|candidate| candidate.path == "/tmp/echonote-missing-python")
        .expect("explicit candidate is included");

        assert!(!candidate.valid);
        assert!(candidate.error.is_some());
    }

    #[test]
    fn service_failure_prevents_ready_status() {
        let (status, primary_action, message) = derive_status(
            &ready_steps_with_failed(SetupStepId::Service),
            ServiceStatus::Error,
        );

        assert_eq!(status, SetupStatus::Error);
        assert_eq!(primary_action, SetupPrimaryAction::Retry);
        assert_eq!(message, "EchoNote could not complete setup.");
    }

    #[test]
    fn model_failure_prevents_ready_status() {
        let (status, primary_action, message) = derive_status(
            &ready_steps_with_failed(SetupStepId::Model),
            ServiceStatus::Stopped,
        );

        assert_eq!(status, SetupStatus::Error);
        assert_eq!(primary_action, SetupPrimaryAction::Retry);
        assert_eq!(message, "EchoNote could not complete setup.");
    }

    #[test]
    fn missing_offline_bundle_requires_repair_when_installation_is_incomplete() {
        let (status, primary_action, _) = derive_status(
            &ready_steps_with_failed(SetupStepId::OfflineBundle),
            ServiceStatus::Stopped,
        );

        assert_eq!(status, SetupStatus::RepairRequired);
        assert_eq!(primary_action, SetupPrimaryAction::Repair);
    }

    #[test]
    fn missing_installed_models_prevents_ready_status() {
        let mut steps = ready_steps_with_failed(SetupStepId::Obsidian);
        for step in &mut steps {
            if step.id == SetupStepId::Obsidian {
                step.status = SetupStepStatus::Passed;
            }
            if step.id == SetupStepId::Models {
                step.status = SetupStepStatus::Warning;
            }
        }

        let (status, primary_action, _) = derive_status(&steps, ServiceStatus::Stopped);

        assert_eq!(status, SetupStatus::NotConfigured);
        assert_eq!(primary_action, SetupPrimaryAction::Setup);
    }

    fn temp_service_dir() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("echonote-setup-detector-test-{nonce}"));
        fs::create_dir_all(dir.join("echonote_asr")).expect("create fake package");
        fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"echonote-asr\"\n",
        )
        .expect("write pyproject");
        dir
    }

    fn ready_steps_with_failed(failed_id: SetupStepId) -> Vec<SetupStep> {
        [
            SetupStepId::System,
            SetupStepId::Python,
            SetupStepId::Runtime,
            SetupStepId::OfflineBundle,
            SetupStepId::Dependencies,
            SetupStepId::Models,
            SetupStepId::Port,
            SetupStepId::Service,
            SetupStepId::Model,
            SetupStepId::Obsidian,
        ]
        .into_iter()
        .map(|id| {
            let status = if id == failed_id {
                SetupStepStatus::Failed
            } else {
                SetupStepStatus::Passed
            };
            SetupStep::new(id, format!("{id:?}"), status, "test", true)
        })
        .collect()
    }
}
