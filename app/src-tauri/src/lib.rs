//! EchoNote desktop app: window, tray, global shortcut and IPC around `echonote-core`.

mod commands;
mod dev;
mod edit;
mod error;
mod files;
mod live;
mod log;
mod models;
mod playback;
mod recording;
mod settings;
mod shortcut;
mod state;
mod tray;
mod views;
mod window;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use echonote_core::engine::{Engine, SessionState};
use tauri::{AppHandle, Emitter, Manager, RunEvent};

use crate::error::CommandError;
use crate::log::log;
use crate::settings::Settings;
use crate::state::{AppState, lock};
use crate::views::Delta;

pub fn run() {
    let dev = dev::Dev::from_env();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            window::show_main(app);
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // Used from Rust only (export); the WebView gets no dialog permission.
        .plugin(tauri_plugin_dialog::init())
        .on_menu_event(tray::on_menu)
        .register_asynchronous_uri_scheme_protocol(playback::SCHEME, playback::handle)
        .setup(move |app| setup(app, dev))
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::list_meetings,
            commands::search,
            commands::get_meeting,
            commands::rename_meeting,
            commands::start_recording,
            commands::stop_recording,
            commands::pause_recording,
            commands::resume_recording,
            commands::live_snapshot,
            commands::get_settings,
            commands::set_settings,
            commands::list_input_devices,
            commands::permissions,
            commands::open_privacy_settings,
            commands::smoke_report,
            edit::edit_segment,
            edit::revert_segment,
            edit::restore_segment,
            edit::list_glossary,
            edit::save_glossary_entry,
            edit::delete_glossary_entry,
            files::export_meeting,
            files::delete_meeting,
            files::storage_usage,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build EchoNote")
        .run(on_run_event);
}

fn setup(app: &mut tauri::App, dev: dev::Dev) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let data_dir = match &dev.data_dir {
        Some(dir) => dir.clone(),
        None => app.path().app_data_dir()?,
    };
    std::fs::create_dir_all(&data_dir)?;
    log::init(&data_dir);
    log!("start {}", app.package_info().version);
    let models = models::locate(&models::roots(&data_dir, app.path().resource_dir().ok()));
    log!(
        "models: asr {}, vad {}",
        models.asr.is_some(),
        models.vad.is_some()
    );
    let config = models::engine_config(data_dir.clone(), &models);
    let (engine, events, settings) = match Engine::open(config) {
        Ok((engine, events)) => {
            let settings = engine
                .store()
                .reader()
                .map(|store| Settings::load(&store))
                .unwrap_or_default();
            (Ok(Arc::new(engine)), Some(events), settings)
        }
        Err(error) => {
            log!("engine: {error}");
            (Err(CommandError::from(error)), None, Settings::default())
        }
    };
    let state = AppState::new(engine, data_dir, models, settings.clone(), dev);
    if state.engine.is_ok()
        && let Err(error) = state.reload_glossary()
    {
        log!("glossary: {error}");
    }
    let live = state.live.clone();
    let recover = state.engine.as_ref().ok().cloned();
    app.manage(state);

    app.manage(tray::create(&handle)?);
    #[cfg(target_os = "macos")]
    app.set_menu(tray::app_menu(&handle, &settings.language)?)?;
    if let Err(error) = shortcut::register(&handle, &settings.shortcut) {
        log!("shortcut: {}", error.code);
        *lock(&handle.state::<AppState>().shortcut_error) = Some(error.message);
    }
    if let Some(events) = events {
        let sink = AppSink(handle.clone());
        std::thread::Builder::new()
            .name("live-events".into())
            .spawn(move || live::run(events, live, &sink))?;
    }
    if let Some(engine) = recover {
        std::thread::Builder::new()
            .name("recovery".into())
            .spawn(move || match engine.recover() {
                Ok(recovered) if !recovered.is_empty() => {
                    log!("recovered {} meetings", recovered.len());
                }
                Ok(_) => {}
                Err(error) => log!("recovery: {error}"),
            })?;
    }
    window::show_main(&handle);
    Ok(())
}

fn on_run_event(app: &AppHandle, event: RunEvent) {
    match event {
        // The last window closed (Quit exits with a code): keep running in the tray.
        RunEvent::ExitRequested {
            code: None, api, ..
        } => {
            if !app.state::<AppState>().quitting.load(Ordering::Acquire) {
                api.prevent_exit();
            }
        }
        RunEvent::Exit => {
            let state = app.state::<AppState>();
            if recording::is_active(&state) {
                let _ = recording::stop(&state);
            }
            log!("exit");
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } => window::show_main(app),
        _ => {}
    }
}

/// Finish a recording in progress, then exit.
pub(crate) fn quit(app: &AppHandle) {
    app.state::<AppState>()
        .quitting
        .store(true, Ordering::Release);
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        if recording::is_active(&state) {
            let _ = recording::stop(&state);
        }
        app.exit(0);
    });
}

/// Menus in the current language and recording state.
pub(crate) fn refresh_menus(app: &AppHandle) {
    let state = app.state::<AppState>();
    let language = state.settings().language;
    let session = state.engine().ok().and_then(|engine| engine.state());
    if let Some(tray) = app.try_state::<tray::Tray>() {
        tray.update(session.map(|(_, state)| state), &language);
    }
    #[cfg(target_os = "macos")]
    if let Ok(menu) = tray::app_menu(app, &language) {
        let _ = app.set_menu(menu);
    }
}

struct AppSink(AppHandle);

impl live::Sink for AppSink {
    fn delta(&self, delta: &Delta) {
        let glossary = self.0.state::<AppState>().glossary();
        if glossary.is_empty() {
            let _ = self.0.emit("live", delta);
        } else {
            let _ = self.0.emit("live", delta.glossed(&glossary));
        }
    }

    fn state_changed(&self, _meeting: i64, state: SessionState) {
        let language = self.0.state::<AppState>().settings().language;
        if let Some(tray) = self.0.try_state::<tray::Tray>() {
            tray.update(Some(state), &language);
        }
    }

    fn library_changed(&self) {
        let _ = self.0.emit("library-changed", ());
    }
}
