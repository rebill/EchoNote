//! Transcript edits and the glossary. Edits write through the store thread and return the
//! segment as shown; the glossary is reloaded after every change.

use echonote_core::glossary::{self, Entry};
use echonote_core::store::{Source, StoreError};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;
use crate::views::SegmentView;

/// The ASR side caps segments at this length too (`echonote_core::text`).
const MAX_SEGMENT_CHARS: usize = 3000;

pub(crate) fn shown(state: &AppState, segment: i64) -> CommandResult<SegmentView> {
    let glossary = state.glossary();
    let segment = state.read(|store| store.segment(segment))?;
    Ok(SegmentView::from(&segment).glossed(&glossary))
}

fn write(
    state: &AppState,
    task: impl FnOnce(&mut echonote_core::store::Store) -> Result<(), StoreError> + Send + 'static,
) -> CommandResult<()> {
    Ok(state.engine()?.store().call(task)?)
}

/// Replace a segment's text with what the user typed.
#[tauri::command]
pub async fn edit_segment(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    text: String,
) -> CommandResult<SegmentView> {
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err(CommandError::invalid("the text is empty"));
    }
    if text.chars().count() > MAX_SEGMENT_CHARS {
        return Err(CommandError::invalid("the text is too long"));
    }
    let current = shown(&state, id)?;
    if current.text == text {
        return Ok(current);
    }
    write(&state, move |store| {
        store.edit_segment(id, &text, Source::User)
    })?;
    let _ = app.emit("library-changed", ());
    shown(&state, id)
}

/// Go back to the recognized text (revision 1), as a new revision.
#[tauri::command]
pub async fn revert_segment(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> CommandResult<SegmentView> {
    write(&state, move |store| store.restore_revision(id, 1))?;
    let _ = app.emit("library-changed", ());
    shown(&state, id)
}

/// Undo a crosstalk decision: show a hidden line, or put back trimmed words.
#[tauri::command]
pub async fn restore_segment(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> CommandResult<SegmentView> {
    write(&state, move |store| store.restore_crosstalk(id))?;
    let _ = app.emit("library-changed", ());
    shown(&state, id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    id: i64,
    term: String,
    aliases: Vec<String>,
}

impl From<Entry> for EntryView {
    fn from(entry: Entry) -> Self {
        Self {
            id: entry.id,
            term: entry.term,
            aliases: entry.aliases,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryInput {
    id: Option<i64>,
    term: String,
    aliases: Vec<String>,
}

fn entries(state: &AppState) -> CommandResult<Vec<EntryView>> {
    Ok(state
        .reload_glossary()?
        .into_iter()
        .map(EntryView::from)
        .collect())
}

#[tauri::command]
pub async fn list_glossary(state: State<'_, AppState>) -> CommandResult<Vec<EntryView>> {
    entries(&state)
}

/// Add or replace an entry; returns the whole glossary.
#[tauri::command]
pub async fn save_glossary_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    entry: EntryInput,
) -> CommandResult<Vec<EntryView>> {
    if entry.aliases.len() > glossary::MAX_ALIASES * 2 {
        return Err(CommandError::invalid("too many aliases"));
    }
    write(&state, move |store| {
        store
            .save_glossary_entry(entry.id, &entry.term, &entry.aliases)
            .map(drop)
    })?;
    let list = entries(&state)?;
    let _ = app.emit("library-changed", ());
    Ok(list)
}

#[tauri::command]
pub async fn delete_glossary_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> CommandResult<Vec<EntryView>> {
    write(&state, move |store| store.delete_glossary_entry(id))?;
    let list = entries(&state)?;
    let _ = app.emit("library-changed", ());
    Ok(list)
}
