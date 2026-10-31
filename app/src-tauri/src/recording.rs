//! Start and stop, shared by the UI commands, the tray menu and the global shortcut.

use std::path::Path;

use echonote_core::capture::{
    AudioSource, FakeSource, Pace, Permission, input_devices, mic_permission,
    request_mic_permission,
};
use echonote_core::engine::{OpenSource, SessionOptions, SessionState};
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{CommandError, CommandResult};
use crate::log::log;
use crate::settings::Settings;
use crate::state::{AppState, lock};

/// Start recording; blocks while the microphone prompt is open. Returns the meeting id.
pub fn start(state: &AppState) -> CommandResult<i64> {
    let engine = state.engine()?;
    let settings = state.settings();
    let mut options = match &state.dev.fake_input {
        Some(dir) => fake_options(dir)?,
        None => device_options(&settings)?,
    };
    options.auto_stop = settings.auto_stop();
    let meeting = engine.start(options)?;
    log!("recording started");
    Ok(meeting)
}

fn device_options(settings: &Settings) -> CommandResult<SessionOptions> {
    let permission = match mic_permission() {
        Permission::Undetermined => request_mic_permission(),
        known => known,
    };
    if permission == Permission::Denied {
        return Err(CommandError::new(
            "mic-denied",
            "microphone access is denied",
        ));
    }
    // A saved microphone that is not plugged in falls back to the system default.
    let device = settings.mic_device.clone().filter(|id| {
        input_devices().is_ok_and(|devices| devices.iter().any(|device| &device.id == id))
    });
    Ok(SessionOptions::devices(
        String::new(),
        true,
        device,
        settings.record_system,
    ))
}

/// `mic.wav` and `sys.wav` from `dir`, in real time (debug builds only).
fn fake_options(dir: &Path) -> CommandResult<SessionOptions> {
    let open = |name: &str| -> Option<OpenSource> {
        let path = dir.join(name);
        path.is_file().then(|| {
            Box::new(move || {
                FakeSource::from_wav(&path, Pace::Real)
                    .map(|source| Box::new(source) as Box<dyn AudioSource>)
            }) as OpenSource
        })
    };
    let mic = open("mic.wav")
        .ok_or_else(|| CommandError::new("no-device", "fake input has no mic.wav"))?;
    Ok(SessionOptions {
        title: String::new(),
        language: None,
        mic: Some(mic),
        sys: open("sys.wav"),
        auto_stop: None,
    })
}

/// Stop and wait until the rest is transcribed.
pub fn stop(state: &AppState) -> CommandResult<()> {
    state.engine()?.stop()?;
    log!("recording stopped");
    Ok(())
}

pub fn is_active(state: &AppState) -> bool {
    state
        .engine()
        .ok()
        .and_then(|engine| engine.state())
        .is_some_and(|(_, state)| {
            matches!(
                state,
                SessionState::Starting | SessionState::Recording | SessionState::Paused
            )
        })
}

/// Start or stop from the tray or the shortcut, off the calling thread.
pub fn toggle(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let result = if is_active(&state) {
            stop(&state)
        } else {
            start(&state).map(|_| ())
        };
        if let Err(error) = result {
            report(&app, error);
        }
    });
}

/// Pause or resume from the tray.
pub fn toggle_pause(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Ok(engine) = state.engine() else {
        return;
    };
    let result = match engine.state() {
        Some((_, SessionState::Recording)) => engine.pause(),
        Some((_, SessionState::Paused)) => engine.resume(),
        _ => return,
    };
    if let Err(error) = result {
        report(&app.clone(), error.into());
    }
}

/// Show an error from an action that has no window of its own.
pub fn report(app: &AppHandle, error: CommandError) {
    log!("action failed: {}", error.code);
    if app.get_webview_window(crate::window::MAIN).is_some() {
        let _ = app.emit("app-error", &error);
    } else {
        *lock(&app.state::<AppState>().pending_error) = Some(error);
    }
    crate::window::show_main(app);
}
