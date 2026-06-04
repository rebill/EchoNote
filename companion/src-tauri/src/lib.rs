mod commands;
mod discovery;
mod logs;
mod path_resolver;
mod process;
mod settings;
mod setup;
mod setup_detector;
mod setup_installer;
mod setup_types;
mod state;

use tauri::{Manager, RunEvent};

const DISCOVERY_HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

pub fn run() {
    let app = tauri::Builder::default()
        .manage(process::ProcessManagerState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_state,
            commands::get_settings,
            commands::save_settings,
            commands::start_service,
            commands::stop_service,
            commands::restart_service,
            commands::load_model,
            commands::copy_diagnostic_report,
            commands::open_logs_folder,
            commands::detect_setup,
            commands::install_or_repair_runtime,
            commands::start_service_with_defaults,
            commands::reset_setup
        ])
        .build(tauri::generate_context!())
        .expect("error while running EchoNote");

    start_discovery_heartbeat(app.handle().clone());

    app.run(|app_handle, event| {
        if matches!(event, RunEvent::ExitRequested { .. } | RunEvent::Exit) {
            shutdown_asr_on_app_exit(app_handle);
        }
    });
}

fn shutdown_asr_on_app_exit(app: &tauri::AppHandle) {
    let Some(process_manager) = app.try_state::<process::ProcessManagerState>() else {
        return;
    };

    let log_store = logs::LogStore::new(app).ok();
    let Ok(mut process_manager) = process_manager.lock() else {
        return;
    };

    if let Some(log_store) = log_store {
        process_manager.set_log_paths(log_store.companion_log_path(), log_store.asr_log_path());
    }
    process_manager.shutdown_on_app_exit();
}

fn start_discovery_heartbeat(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(DISCOVERY_HEARTBEAT_INTERVAL);
        if let Err(error) = refresh_discovery(&app) {
            if let Ok(log_store) = logs::LogStore::new(&app) {
                let _ = logs::append_log_line(
                    &log_store.companion_log_path(),
                    &format!("Discovery heartbeat failed: {error}"),
                );
            }
        }
    });
}

fn refresh_discovery(app: &tauri::AppHandle) -> Result<(), String> {
    let settings = settings::SettingsStore::new(app)?
        .load_or_default()?
        .settings;
    let log_store = logs::LogStore::new(app)?;
    let process_manager = app
        .try_state::<process::ProcessManagerState>()
        .ok_or_else(|| "Process manager state is unavailable.".to_string())?;
    let runtime = {
        let mut process_manager = process_manager
            .lock()
            .map_err(|_| "Process manager state is unavailable.".to_string())?;
        process_manager.set_log_paths(log_store.companion_log_path(), log_store.asr_log_path());
        process_manager.snapshot(&settings)
    };

    discovery::DiscoveryWriter::new(app)?.write(&runtime)
}
