mod commands;
mod discovery;
mod logs;
mod process;
mod settings;
mod state;

use tauri::{Manager, RunEvent};

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
            commands::open_logs_folder
        ])
        .build(tauri::generate_context!())
        .expect("error while running EchoNote ASR Companion");

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
