//! The global start/stop shortcut.

use std::str::FromStr;

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::error::CommandError;

/// Register `accelerator` (empty: none) to start or stop recording.
pub fn register(app: &AppHandle, accelerator: &str) -> Result<(), CommandError> {
    if accelerator.is_empty() {
        return Ok(());
    }
    let shortcut = Shortcut::from_str(accelerator)
        .map_err(|error| CommandError::new("bad-shortcut", error.to_string()))?;
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _, event| {
            if event.state == ShortcutState::Pressed {
                crate::recording::toggle(app);
            }
        })
        .map_err(|error| CommandError::new("shortcut-taken", error.to_string()))
}

/// Swap `old` for `new`; when `new` cannot be registered, `old` stays.
pub fn replace(app: &AppHandle, old: &str, new: &str) -> Result<(), CommandError> {
    if old == new {
        return Ok(());
    }
    if let Ok(shortcut) = Shortcut::from_str(old) {
        let _ = app.global_shortcut().unregister(shortcut);
    }
    register(app, new).inspect_err(|_| {
        let _ = register(app, old);
    })
}
