use crate::discovery::DiscoveryWriter;
use crate::logs::LogStore;
use crate::process::ProcessManagerState;
use crate::settings::{CompanionSettings, SettingsResponse, SettingsStore};
use crate::setup_detector;
use crate::setup_installer;
use crate::setup_types::{
    SetupPrimaryAction, SetupResponse, SetupStatus, SetupStep, SetupStepId, SetupStepStatus,
};
use crate::state::{CompanionAppState, ServiceStatus};

pub fn detect_setup(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<SetupResponse, String> {
    let settings_response = SettingsStore::new(&app)?.load_read_only_or_default();
    setup_response_from_settings(&app, &process_manager, settings_response)
}

pub fn install_or_repair_runtime(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<SetupResponse, String> {
    let settings_response = SettingsStore::new(&app)?.load_read_only_or_default();
    let runtime = runtime_snapshot(&process_manager, &settings_response.settings)?;
    let detection = setup_detector::detect(settings_response.settings, &runtime);
    let failure_settings = detection.settings.clone();
    let detection = match setup_installer::install_or_repair(&app, &process_manager, detection) {
        Ok(detection) => detection,
        Err(error) => return setup_error_response(&app, &process_manager, failure_settings, error),
    };
    let runtime = runtime_snapshot(&process_manager, &detection.settings)?;
    if runtime.service_status == ServiceStatus::Running {
        DiscoveryWriter::new(&app)?.write(&runtime)?;
    }
    let state = app_state_from_runtime(
        &app,
        SettingsStore::new(&app)?.settings_path_string(),
        runtime,
    )?;
    Ok(detection.response(state))
}

pub fn start_service_with_defaults(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<SetupResponse, String> {
    let store = SettingsStore::new(&app)?;
    let settings_response = store.load_or_default()?;
    let log_store = LogStore::new(&app)?;
    {
        let mut process_manager = process_manager
            .lock()
            .map_err(|_| "Process manager state is unavailable.".to_string())?;
        process_manager.set_log_paths(log_store.companion_log_path(), log_store.asr_log_path());
        process_manager.start(&settings_response.settings);
    }

    let runtime = runtime_snapshot(&process_manager, &settings_response.settings)?;
    if runtime.service_status == ServiceStatus::Running {
        DiscoveryWriter::new(&app)?.write(&runtime)?;
    }
    let state = app_state_from_runtime(&app, settings_response.settings_path, runtime.clone())?;
    let detection = setup_detector::detect(settings_response.settings, &runtime);
    Ok(detection.response(state))
}

pub fn reset_setup(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<SetupResponse, String> {
    let settings = setup_installer::reset_settings(&app)?;
    let settings_path = SettingsStore::new(&app)?.settings_path_string();
    setup_response_from_settings(
        &app,
        &process_manager,
        SettingsResponse {
            settings,
            settings_path,
            recovered: false,
        },
    )
}

pub fn setup_diagnostic_lines(
    app: &tauri::AppHandle,
    settings: &CompanionSettings,
    runtime: &crate::state::RuntimeState,
) -> Vec<String> {
    let detection = setup_detector::detect(settings.clone(), runtime);
    let mut lines = vec![
        format!("- Setup status: {:?}", detection.status),
        format!("- Setup primary action: {:?}", detection.primary_action),
        format!("- Setup message: {}", detection.message),
    ];
    for step in detection.steps {
        lines.push(format!(
            "- Setup step {:?}: {:?} - {}",
            step.id, step.status, step.summary
        ));
    }
    if !detection.python_candidates.is_empty() {
        lines.push("- Python candidates:".to_string());
        for candidate in detection.python_candidates {
            lines.push(format!(
                "  - {}: {}{}",
                candidate.path,
                if candidate.valid { "valid" } else { "invalid" },
                candidate
                    .version
                    .as_ref()
                    .map(|version| format!(" ({version})"))
                    .or(candidate.error.map(|error| format!(" ({error})")))
                    .unwrap_or_default()
            ));
        }
    }
    if let Ok(log_store) = LogStore::new(app) {
        lines.push(format!(
            "- Setup logs path: {}",
            log_store.logs_dir_string()
        ));
    }
    lines
}

fn setup_response_from_settings(
    app: &tauri::AppHandle,
    process_manager: &tauri::State<'_, ProcessManagerState>,
    settings_response: SettingsResponse,
) -> Result<SetupResponse, String> {
    let runtime = runtime_snapshot(&process_manager, &settings_response.settings)?;
    let state = app_state_from_runtime(app, settings_response.settings_path, runtime.clone())?;
    let mut detection = setup_detector::detect(settings_response.settings, &runtime);
    if detection.status == SetupStatus::Ready && runtime.service_status == ServiceStatus::Running {
        detection.status = SetupStatus::Running;
        detection.primary_action = SetupPrimaryAction::Stop;
        detection.message = "Local transcription is running.".to_string();
    }
    Ok(detection.response(state))
}

fn setup_error_response(
    app: &tauri::AppHandle,
    process_manager: &tauri::State<'_, ProcessManagerState>,
    settings: CompanionSettings,
    error: setup_installer::SetupInstallError,
) -> Result<SetupResponse, String> {
    let runtime = runtime_snapshot(process_manager, &settings)?;
    let state = app_state_from_runtime(
        app,
        SettingsStore::new(app)?.settings_path_string(),
        runtime.clone(),
    )?;
    let mut detection = setup_detector::detect(settings, &runtime);
    mark_failed_step(&mut detection.steps, error.step_id, &error.message);
    detection.status = SetupStatus::Error;
    detection.primary_action = SetupPrimaryAction::Retry;
    detection.message =
        "EchoNote could not complete setup. Check the failed step below.".to_string();
    Ok(detection.response(state))
}

fn mark_failed_step(steps: &mut [SetupStep], step_id: SetupStepId, message: &str) {
    if let Some(step) = steps.iter_mut().find(|step| step.id == step_id) {
        step.status = SetupStepStatus::Failed;
        step.summary = setup_failure_summary(step_id).to_string();
        step.detail = Some(message.to_string());
        step.recoverable = true;
    }
}

fn setup_failure_summary(step_id: SetupStepId) -> &'static str {
    match step_id {
        SetupStepId::Python => "EchoNote could not prepare Python.",
        SetupStepId::Runtime => "EchoNote could not prepare the ASR runtime.",
        SetupStepId::Dependencies => "EchoNote could not install ASR dependencies.",
        SetupStepId::Service => "EchoNote could not start the local service.",
        _ => "EchoNote setup failed.",
    }
}

fn app_state_from_runtime(
    app: &tauri::AppHandle,
    settings_path: String,
    runtime: crate::state::RuntimeState,
) -> Result<CompanionAppState, String> {
    let discovery_writer = DiscoveryWriter::new(app)?;
    let log_store = LogStore::new(app)?;
    Ok(CompanionAppState::from_runtime(
        settings_path,
        discovery_writer.path_string(),
        log_store.logs_dir_string(),
        runtime,
    ))
}

fn runtime_snapshot(
    process_manager: &tauri::State<'_, ProcessManagerState>,
    settings: &CompanionSettings,
) -> Result<crate::state::RuntimeState, String> {
    let runtime = process_manager
        .lock()
        .map_err(|_| "Process manager state is unavailable.".to_string())?
        .snapshot(settings);
    Ok(runtime)
}
