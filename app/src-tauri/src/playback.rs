//! `echonote-audio://localhost/<meeting id>`: the meeting's tracks mixed into a WAV, decoded
//! from the Ogg/Opus files for each byte range the player asks for. Only meeting ids are
//! accepted; files are looked up in the library, never taken from the URL.

use echonote_core::recording::playback::{MixedWav, parse_range};
use tauri::http::{Request, Response, StatusCode, header};
use tauri::{AppHandle, Manager, UriSchemeContext, UriSchemeResponder, Wry};

use crate::log::log;
use crate::recording;
use crate::state::AppState;

pub const SCHEME: &str = "echonote-audio";

pub fn handle(
    context: UriSchemeContext<'_, Wry>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = context.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || responder.respond(respond(&app, &request)));
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(code)
        .body(Vec::new())
        .expect("static response")
}

fn respond(app: &AppHandle, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let Ok(id) = request.uri().path().trim_start_matches('/').parse::<i64>() else {
        return status(StatusCode::BAD_REQUEST);
    };
    let state = app.state::<AppState>();
    // WebKit plays audio from its own process, which the system-audio tap would record.
    if recording::is_active(&state) {
        return status(StatusCode::CONFLICT);
    }
    let Ok(engine) = state.engine() else {
        return status(StatusCode::SERVICE_UNAVAILABLE);
    };
    let tracks = match state.read(|store| store.tracks(id)) {
        Ok(tracks) if !tracks.is_empty() => tracks,
        _ => return status(StatusCode::NOT_FOUND),
    };
    let dir = engine.config().meeting_dir(id);
    let duration_ms = tracks
        .iter()
        .filter_map(|track| track.duration_ms)
        .max()
        .unwrap_or(0);
    let wav = MixedWav {
        tracks: tracks
            .iter()
            .filter_map(|track| std::path::Path::new(&track.file).file_name())
            .map(|name| dir.join(name))
            .collect(),
        samples: (duration_ms.max(0) as u64) * u64::from(echonote_core::SAMPLE_RATE) / 1_000,
    };
    let len = wav.len();
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());
    let Ok((start, end)) = parse_range(range, len) else {
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{len}"))
            .body(Vec::new())
            .expect("static response");
    };
    let body = match wav.read(start, end) {
        Ok(body) => body,
        Err(error) => {
            log!("playback: {error}");
            return status(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    let whole = range.is_none() && end + 1 == len;
    Response::builder()
        .status(if whole {
            StatusCode::OK
        } else {
            StatusCode::PARTIAL_CONTENT
        })
        .header(header::CONTENT_TYPE, "audio/wav")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_LENGTH, body.len())
        .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
        .body(body)
        .expect("valid headers")
}
