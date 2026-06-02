use crate::discovery::DiscoveryWriter;
use crate::logs::{diagnostic_report, LogStore};
use crate::process::ProcessManagerState;
use crate::settings::{CompanionSettings, SettingsResponse, SettingsStore};
use crate::setup;
use crate::setup_types::SetupResponse;
use crate::state::CompanionAppState;

#[tauri::command]
pub fn get_app_state(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<CompanionAppState, String> {
    current_app_state(app, process_manager)
}

#[tauri::command]
pub fn get_settings(app: tauri::AppHandle) -> Result<SettingsResponse, String> {
    SettingsStore::new(&app)?.load_or_default()
}

#[tauri::command]
pub fn save_settings(
    app: tauri::AppHandle,
    settings: CompanionSettings,
) -> Result<SettingsResponse, String> {
    SettingsStore::new(&app)?.save(settings)
}

#[tauri::command]
pub fn start_service(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<CompanionAppState, String> {
    let store = SettingsStore::new(&app)?;
    let response = store.load_or_default()?;
    let log_store = LogStore::new(&app)?;
    {
        let mut process_manager = process_manager
            .lock()
            .map_err(|_| "Process manager state is unavailable.".to_string())?;
        process_manager.set_log_paths(log_store.companion_log_path(), log_store.asr_log_path());
        process_manager.start(&response.settings);
    }
    app_state_from_response(&app, process_manager, response)
}

#[tauri::command]
pub fn stop_service(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<CompanionAppState, String> {
    let store = SettingsStore::new(&app)?;
    let response = store.load_or_default()?;
    let log_store = LogStore::new(&app)?;
    {
        let mut process_manager = process_manager
            .lock()
            .map_err(|_| "Process manager state is unavailable.".to_string())?;
        process_manager.set_log_paths(log_store.companion_log_path(), log_store.asr_log_path());
        process_manager.stop();
    }
    app_state_from_response(&app, process_manager, response)
}

#[tauri::command]
pub fn restart_service(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<CompanionAppState, String> {
    let store = SettingsStore::new(&app)?;
    let response = store.load_or_default()?;
    let log_store = LogStore::new(&app)?;
    {
        let mut process_manager = process_manager
            .lock()
            .map_err(|_| "Process manager state is unavailable.".to_string())?;
        process_manager.set_log_paths(log_store.companion_log_path(), log_store.asr_log_path());
        process_manager.restart(&response.settings);
    }
    app_state_from_response(&app, process_manager, response)
}

#[tauri::command]
pub fn load_model(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<CompanionAppState, String> {
    let store = SettingsStore::new(&app)?;
    let response = store.load_or_default()?;
    let log_store = LogStore::new(&app)?;
    {
        let mut process_manager = process_manager
            .lock()
            .map_err(|_| "Process manager state is unavailable.".to_string())?;
        process_manager.set_log_paths(log_store.companion_log_path(), log_store.asr_log_path());
        process_manager.load_model(&response.settings);
    }
    app_state_from_response(&app, process_manager, response)
}

#[tauri::command]
pub fn copy_diagnostic_report(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<String, String> {
    diagnostic_report(&app, process_manager)
}

#[tauri::command]
pub fn open_logs_folder(app: tauri::AppHandle) -> Result<(), String> {
    LogStore::new(&app)?.open_logs_folder()
}

#[tauri::command]
pub fn detect_setup(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<SetupResponse, String> {
    setup::detect_setup(app, process_manager)
}

#[tauri::command]
pub fn install_or_repair_runtime(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<SetupResponse, String> {
    setup::install_or_repair_runtime(app, process_manager)
}

#[tauri::command]
pub fn start_service_with_defaults(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<SetupResponse, String> {
    setup::start_service_with_defaults(app, process_manager)
}

#[tauri::command]
pub fn reset_setup(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<SetupResponse, String> {
    setup::reset_setup(app, process_manager)
}

fn current_app_state(
    app: tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
) -> Result<CompanionAppState, String> {
    let store = SettingsStore::new(&app)?;
    let response = store.load_or_default()?;
    app_state_from_response(&app, process_manager, response)
}

fn app_state_from_response(
    app: &tauri::AppHandle,
    process_manager: tauri::State<'_, ProcessManagerState>,
    response: SettingsResponse,
) -> Result<CompanionAppState, String> {
    let log_store = LogStore::new(app)?;
    let runtime = {
        let mut process_manager = process_manager
            .lock()
            .map_err(|_| "Process manager state is unavailable.".to_string())?;
        process_manager.set_log_paths(log_store.companion_log_path(), log_store.asr_log_path());
        process_manager.snapshot(&response.settings)
    };
    let discovery_writer = DiscoveryWriter::new(app)?;
    discovery_writer.write(&runtime)?;

    Ok(CompanionAppState::from_runtime(
        response.settings_path,
        discovery_writer.path_string(),
        log_store.logs_dir_string(),
        runtime,
    ))
}
