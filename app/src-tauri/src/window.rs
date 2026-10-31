//! The main window. Closing it destroys the web view (the app keeps running in the tray with
//! little memory); opening it again builds a new one from the config.

use tauri::{AppHandle, Manager, WebviewWindowBuilder};

use crate::log::log;

pub const MAIN: &str = "main";

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    // Building a window inside an event handler deadlocks on Windows.
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(config) = app
            .config()
            .app
            .windows
            .iter()
            .find(|window| window.label == MAIN)
            .cloned()
        else {
            return;
        };
        match WebviewWindowBuilder::from_config(&app, &config).and_then(|builder| builder.build()) {
            Ok(window) => {
                let _ = window.set_focus();
            }
            Err(error) => log!("main window: {error}"),
        }
    });
}
