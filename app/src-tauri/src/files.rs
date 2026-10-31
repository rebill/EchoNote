//! Export, delete and storage usage. The save dialog is opened from Rust: the WebView has no
//! dialog or file permission and never sends a path, only a meeting id and a format.

use std::path::PathBuf;

use echonote_core::export::{Export, Format, Language, MAX_OFFSET_MINUTES};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::commands::blocking;
use crate::error::{CommandError, CommandResult};
use crate::log::log;
use crate::state::AppState;

/// Library setting: the folder of the last export.
const EXPORT_DIR_KEY: &str = "export.dir";

fn format(name: &str) -> CommandResult<Format> {
    match name {
        "markdown" => Ok(Format::Markdown),
        "text" => Ok(Format::Text),
        _ => Err(CommandError::invalid("unknown export format")),
    }
}

/// The folder the save dialog opens in: the last one used, else Documents.
fn start_dir(app: &AppHandle, state: &AppState) -> Option<PathBuf> {
    let last = state
        .read(|store| store.setting(EXPORT_DIR_KEY))
        .ok()
        .flatten()
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir());
    last.or_else(|| app.path().document_dir().ok())
}

/// Ask where to save; `None` when the user cancels. Debug builds can skip the dialog with
/// `ECHONOTE_EXPORT_DIR` (smoke run).
fn choose(
    app: &AppHandle,
    state: &AppState,
    file_name: &str,
    format: Format,
) -> CommandResult<Option<PathBuf>> {
    if let Some(dir) = &state.dev.export_dir {
        return Ok(Some(dir.join(file_name)));
    }
    let mut dialog = app
        .dialog()
        .file()
        .set_file_name(file_name)
        .set_can_create_directories(true);
    dialog = match format {
        Format::Markdown => dialog.add_filter("Markdown", &["md"]),
        Format::Text => dialog.add_filter("Text", &["txt"]),
    };
    if let Some(dir) = start_dir(app, state) {
        dialog = dialog.set_directory(dir);
    }
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    match dialog.blocking_save_file() {
        None => Ok(None),
        Some(path) => path
            .into_path()
            .map(Some)
            .map_err(|_| CommandError::invalid("the chosen location is not a file")),
    }
}

/// Write the meeting as Markdown or text where the user chooses; returns the path, or `None`
/// when cancelled. `utc_offset_minutes` is the UI's offset at the meeting's start.
#[tauri::command]
pub async fn export_meeting(
    app: AppHandle,
    id: i64,
    format: String,
    utc_offset_minutes: i32,
) -> CommandResult<Option<String>> {
    let format = self::format(&format)?;
    if utc_offset_minutes.abs() > MAX_OFFSET_MINUTES {
        return Err(CommandError::invalid("time zone offset is out of range"));
    }
    let handle = app.clone();
    blocking(app, move |state| {
        let glossary = state.glossary();
        let (meeting, segments) =
            state.read(|store| Ok((store.meeting(id)?, store.segments(id)?)))?;
        let language = match state.settings().language.as_str() {
            "en" => Language::En,
            _ => Language::Zh,
        };
        let export = Export {
            meeting: &meeting,
            segments: &segments,
            glossary: &glossary,
            language,
            utc_offset_minutes,
        };
        let Some(path) = choose(&handle, state, &export.file_name(format), format)? else {
            return Ok(None);
        };
        std::fs::write(&path, export.render(format))
            .map_err(|error| CommandError::new("export-failed", error.kind().to_string()))?;
        log!("exported a meeting as {}", format.extension());
        if let Some(dir) = path
            .parent()
            .and_then(|dir| dir.to_str())
            .map(str::to_owned)
        {
            let _ = state
                .engine()?
                .store()
                .call(move |store| store.set_setting(EXPORT_DIR_KEY, &dir));
        }
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
}

/// Delete a meeting with its recordings, transcript and history.
#[tauri::command]
pub async fn delete_meeting(app: AppHandle, id: i64) -> CommandResult<()> {
    let handle = app.clone();
    blocking(app, move |state| {
        state.engine()?.delete_meeting(id)?;
        log!("deleted a meeting");
        let _ = handle.emit("library-changed", ());
        Ok(())
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageView {
    library_bytes: u64,
    audio_bytes: u64,
}

#[tauri::command]
pub async fn storage_usage(app: AppHandle) -> CommandResult<StorageView> {
    blocking(app, |state| {
        let usage = state.engine()?.storage_usage();
        Ok(StorageView {
            library_bytes: usage.library_bytes,
            audio_bytes: usage.audio_bytes,
        })
    })
    .await
}
