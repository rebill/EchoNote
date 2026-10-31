//! IPC commands. Each one is allowed by name in `capabilities/main.json`; arguments are ids
//! and short strings, never paths.

use echonote_core::capture::{Permission, input_devices, mic_permission, sys_permission};
use echonote_core::store::{Meeting, Track};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::{CommandError, CommandResult};
use crate::state::{AppState, lock};
use crate::views::{SegmentView, Snapshot};

const MAX_TITLE: usize = 200;
const MAX_QUERY: usize = 200;
const SEARCH_LIMIT: usize = 200;

/// Run blocking work off the async runtime.
pub async fn blocking<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppState) -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(move || work(&app.state::<AppState>()))
        .await
        .map_err(|error| CommandError::new("engine", error.to_string()))?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    platform: &'static str,
    data_dir: String,
    model_installed: bool,
    vad_installed: bool,
    engine_error: Option<CommandError>,
    shortcut_error: Option<String>,
    /// An error from a tray or shortcut action while no window was open.
    pending_error: Option<CommandError>,
    smoke: bool,
}

#[tauri::command]
pub async fn app_info(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppInfo> {
    let platform = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "other"
    };
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        platform,
        data_dir: state.data_dir.display().to_string(),
        model_installed: state.models.asr.is_some(),
        vad_installed: state.models.vad.is_some(),
        engine_error: state.engine.as_ref().err().cloned(),
        shortcut_error: lock(&state.shortcut_error).clone(),
        pending_error: lock(&state.pending_error).take(),
        smoke: state.dev.smoke.is_some(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingView {
    id: i64,
    title: String,
    started_at_ms: i64,
    ended_at_ms: Option<i64>,
    duration_ms: Option<i64>,
    status: &'static str,
    language: Option<String>,
}

impl From<Meeting> for MeetingView {
    fn from(meeting: Meeting) -> Self {
        Self {
            id: meeting.id,
            title: meeting.title,
            started_at_ms: meeting.started_at_ms,
            ended_at_ms: meeting.ended_at_ms,
            duration_ms: meeting.duration_ms,
            status: meeting.status.as_str(),
            language: meeting.language,
        }
    }
}

#[tauri::command]
pub async fn list_meetings(state: State<'_, AppState>) -> CommandResult<Vec<MeetingView>> {
    let meetings = state.read(|store| store.meetings())?;
    Ok(meetings.into_iter().map(MeetingView::from).collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHitView {
    meeting_id: i64,
    meeting_title: String,
    segment_id: i64,
    track: &'static str,
    start_ms: i64,
    text: String,
}

#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    meeting_id: Option<i64>,
) -> CommandResult<Vec<SearchHitView>> {
    if query.len() > MAX_QUERY {
        return Err(CommandError::invalid("query is too long"));
    }
    // A term also finds the misrecognized spellings it is shown for.
    let glossary = state.glossary();
    let queries = glossary.search_variants(&query);
    let hits = state.read(|store| store.search_any(&queries, meeting_id, SEARCH_LIMIT))?;
    Ok(hits
        .into_iter()
        .map(|hit| SearchHitView {
            meeting_id: hit.meeting_id,
            meeting_title: hit.meeting_title,
            segment_id: hit.segment_id,
            track: hit.track.as_str(),
            start_ms: hit.start_ms,
            text: glossary.apply(&hit.text).into_owned(),
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackView {
    kind: &'static str,
    device_name: Option<String>,
    duration_ms: Option<i64>,
}

impl From<Track> for TrackView {
    fn from(track: Track) -> Self {
        Self {
            kind: track.kind.as_str(),
            device_name: track.device_name,
            duration_ms: track.duration_ms,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDetail {
    meeting: MeetingView,
    tracks: Vec<TrackView>,
    segments: Vec<SegmentView>,
    /// Size of the recordings on disk.
    audio_bytes: u64,
}

#[tauri::command]
pub async fn get_meeting(state: State<'_, AppState>, id: i64) -> CommandResult<MeetingDetail> {
    let glossary = state.glossary();
    let audio_bytes = state.engine()?.meeting_bytes(id);
    state.read(|store| {
        Ok(MeetingDetail {
            audio_bytes,
            meeting: store.meeting(id)?.into(),
            tracks: store.tracks(id)?.into_iter().map(TrackView::from).collect(),
            segments: store
                .segments(id)?
                .iter()
                .map(|segment| SegmentView::from(segment).glossed(&glossary))
                .collect(),
        })
    })
}

#[tauri::command]
pub async fn rename_meeting(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    title: String,
) -> CommandResult<()> {
    let title = title.trim().to_owned();
    if title.chars().count() > MAX_TITLE {
        return Err(CommandError::invalid("title is too long"));
    }
    state
        .engine()?
        .store()
        .call(move |store| store.rename_meeting(id, &title))?;
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn start_recording(app: AppHandle) -> CommandResult<i64> {
    blocking(app, crate::recording::start).await
}

#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> CommandResult<()> {
    blocking(app, crate::recording::stop).await
}

#[tauri::command]
pub async fn pause_recording(state: State<'_, AppState>) -> CommandResult<()> {
    Ok(state.engine()?.pause()?)
}

#[tauri::command]
pub async fn resume_recording(state: State<'_, AppState>) -> CommandResult<()> {
    Ok(state.engine()?.resume()?)
}

/// The recording in progress, for a window opened during it.
#[tauri::command]
pub async fn live_snapshot(state: State<'_, AppState>) -> CommandResult<Option<Snapshot>> {
    let snapshot = lock(&state.live).snapshot(std::time::Instant::now());
    let Some(mut snapshot) = snapshot else {
        return Ok(None);
    };
    let glossary = state.glossary();
    snapshot.segments = state
        .read(|store| store.segments(snapshot.meeting))?
        .iter()
        .map(|segment| SegmentView::from(segment).glossed(&glossary))
        .collect();
    for preview in &mut snapshot.previews {
        *preview = glossary.apply(preview).into_owned();
    }
    Ok(Some(snapshot))
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> CommandResult<crate::settings::Settings> {
    Ok(state.settings())
}

#[tauri::command]
pub async fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: crate::settings::Settings,
) -> CommandResult<()> {
    settings.validate()?;
    let old = state.settings();
    if old.shortcut != settings.shortcut {
        crate::shortcut::replace(&app, &old.shortcut, &settings.shortcut)?;
        *lock(&state.shortcut_error) = None;
    }
    let json = settings.to_json();
    state
        .engine()?
        .store()
        .call(move |store| store.set_setting(crate::settings::KEY, &json))?;
    let language = settings.language.clone();
    *lock(&state.settings) = settings;
    if old.language != language {
        crate::refresh_menus(&app);
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    id: String,
    name: String,
    is_default: bool,
}

#[tauri::command]
pub async fn list_input_devices(app: AppHandle) -> CommandResult<Vec<DeviceView>> {
    blocking(app, |_| {
        let devices =
            input_devices().map_err(|error| CommandError::new("capture", error.to_string()))?;
        Ok(devices
            .into_iter()
            .map(|device| DeviceView {
                id: device.id,
                name: device.name,
                is_default: device.is_default,
            })
            .collect())
    })
    .await
}

fn permission(permission: Permission) -> &'static str {
    match permission {
        Permission::Granted => "granted",
        Permission::Denied => "denied",
        Permission::Undetermined => "undetermined",
    }
}

#[derive(Serialize)]
pub struct PermissionsView {
    mic: &'static str,
    sys: &'static str,
}

#[tauri::command]
pub async fn permissions() -> CommandResult<PermissionsView> {
    Ok(PermissionsView {
        mic: permission(mic_permission()),
        sys: permission(sys_permission()),
    })
}

/// Open the system privacy pane for `kind` (`mic` or `sys`).
#[tauri::command]
pub async fn open_privacy_settings(kind: String) -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    let (program, target) = (
        "/usr/bin/open",
        match kind.as_str() {
            "mic" => "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
            "sys" => "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture",
            _ => return Err(CommandError::invalid("unknown privacy pane")),
        },
    );
    #[cfg(target_os = "windows")]
    let (program, target) = (
        "explorer.exe",
        match kind.as_str() {
            "mic" => "ms-settings:privacy-microphone",
            _ => return Err(CommandError::invalid("unknown privacy pane")),
        },
    );
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let (program, target): (&str, &str) = {
        let _ = kind;
        return Err(CommandError::invalid("not supported on this platform"));
    };
    // explorer.exe exits with 1 even when it opened the page, so the status is not checked.
    std::process::Command::new(program)
        .arg(target)
        .spawn()
        .map(drop)
        .map_err(|error| CommandError::new("io", error.to_string()))
}

/// Debug smoke run (`ECHONOTE_SMOKE`): write the UI's report and quit. Fails otherwise.
#[tauri::command]
pub async fn smoke_report(
    app: AppHandle,
    state: State<'_, AppState>,
    report: serde_json::Value,
) -> CommandResult<()> {
    let Some(path) = state.dev.smoke.clone() else {
        return Err(CommandError::invalid("not a smoke run"));
    };
    let json = serde_json::to_string_pretty(&report)
        .map_err(|error| CommandError::invalid(error.to_string()))?;
    std::fs::write(&path, json).map_err(|error| CommandError::new("io", error.to_string()))?;
    crate::quit(&app);
    Ok(())
}
