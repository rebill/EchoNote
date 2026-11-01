//! Tray (menu bar) icon and its menu, and the macOS app menu.

use echonote_core::engine::SessionState;
use tauri::image::Image;
#[cfg(target_os = "macos")]
use tauri::menu::Submenu;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Wry};

use crate::recording;
use crate::state::AppState;

pub struct Tray {
    icon: TrayIcon<Wry>,
    toggle: MenuItem<Wry>,
    pause: MenuItem<Wry>,
    open: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

/// Menu texts in the UI language.
fn text(language: &str, key: &str) -> &'static str {
    let en = language == "en";
    match key {
        "start" => {
            if en {
                "Start recording"
            } else {
                "开始录音"
            }
        }
        "stop" => {
            if en {
                "Stop recording"
            } else {
                "停止录音"
            }
        }
        "pause" => {
            if en {
                "Pause"
            } else {
                "暂停"
            }
        }
        "resume" => {
            if en {
                "Resume"
            } else {
                "继续"
            }
        }
        "open" => {
            if en {
                "Open EchoNote"
            } else {
                "打开 EchoNote"
            }
        }
        "quit" => {
            if en {
                "Quit EchoNote"
            } else {
                "退出 EchoNote"
            }
        }
        "recording" => {
            if en {
                "EchoNote: recording"
            } else {
                "EchoNote：正在录音"
            }
        }
        "paused" => {
            if en {
                "EchoNote: paused"
            } else {
                "EchoNote：已暂停"
            }
        }
        "edit" => {
            if en {
                "Edit"
            } else {
                "编辑"
            }
        }
        "window" => {
            if en {
                "Window"
            } else {
                "窗口"
            }
        }
        _ => "EchoNote",
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<Tray> {
    let language = app.state::<AppState>().settings().language;
    let item =
        |id: &str, key: &str| MenuItem::with_id(app, id, text(&language, key), true, None::<&str>);
    let toggle = item("toggle", "start")?;
    let pause = item("pause", "pause")?;
    pause.set_enabled(false)?;
    let open = item("open", "open")?;
    let quit = item("quit", "quit")?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle, &pause, &separator, &open, &quit])?;
    // macOS shows a monochrome template glyph; Windows the colour app icon.
    let (image, template) = if cfg!(target_os = "macos") {
        (
            Image::from_bytes(include_bytes!("../icons/tray-template.png"))?,
            true,
        )
    } else {
        (
            Image::from_bytes(include_bytes!("../icons/32x32.png"))?,
            false,
        )
    };
    let icon = TrayIconBuilder::with_id("main")
        .icon(image)
        .icon_as_template(template)
        .tooltip("EchoNote")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .build(app)?;
    Ok(Tray {
        icon,
        toggle,
        pause,
        open,
        quit,
    })
}

impl Tray {
    /// Match the menu to the recording state and the UI language.
    pub fn update(&self, state: Option<SessionState>, language: &str) {
        let active = matches!(
            state,
            Some(SessionState::Starting | SessionState::Recording | SessionState::Paused)
        );
        let paused = state == Some(SessionState::Paused);
        let _ = self
            .toggle
            .set_text(text(language, if active { "stop" } else { "start" }));
        let _ = self
            .toggle
            .set_enabled(state != Some(SessionState::Stopping));
        let _ = self
            .pause
            .set_text(text(language, if paused { "resume" } else { "pause" }));
        let _ = self.pause.set_enabled(active);
        let _ = self.open.set_text(text(language, "open"));
        let _ = self.quit.set_text(text(language, "quit"));
        let tooltip = match state {
            Some(SessionState::Recording) => text(language, "recording"),
            Some(SessionState::Paused) => text(language, "paused"),
            _ => "EchoNote",
        };
        let _ = self.icon.set_tooltip(Some(tooltip));
        #[cfg(target_os = "macos")]
        {
            let title = match state {
                Some(SessionState::Recording) => Some("●"),
                Some(SessionState::Paused) => Some("❚❚"),
                _ => None,
            };
            let _ = self.icon.set_title(title);
        }
    }
}

/// Menu clicks from the tray and the app menu.
pub fn on_menu(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "toggle" => recording::toggle(app),
        "pause" => recording::toggle_pause(app),
        "open" => crate::window::show_main(app),
        "quit" | "app-quit" => crate::quit(app),
        _ => {}
    }
}

/// The macOS app menu: Quit goes through `quit` so a recording is finished first, and Edit
/// keeps copy and paste working in text fields.
#[cfg(target_os = "macos")]
pub fn app_menu(app: &AppHandle, language: &str) -> tauri::Result<Menu<Wry>> {
    let quit = MenuItem::with_id(
        app,
        "app-quit",
        text(language, "quit"),
        true,
        Some("CmdOrCtrl+Q"),
    )?;
    let app_menu = Submenu::with_items(
        app,
        "EchoNote",
        true,
        &[
            &PredefinedMenuItem::about(app, None, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        text(language, "edit"),
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        text(language, "window"),
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    Menu::with_items(app, &[&app_menu, &edit, &window])
}
