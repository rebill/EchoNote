//! Transcribe a saved meeting again with the high-accuracy model, and undo that. It runs as
//! a job like the AI runs (progress on the `ai` event, cancelled the same way); starting a
//! recording stops it, and what was decoded until then is kept.

use std::sync::Arc;

use echonote_core::asr::{ModelKind, OfflineModelSpec};
use echonote_core::engine::{Model, SessionState, SherpaModel};
use echonote_models::{Manifest, Role};
use tauri::{AppHandle, Emitter, State};

use crate::ai::jobs::{self, Kind, Out, Stage};
use crate::error::{CommandError, CommandResult};
use crate::log::log;
use crate::state::AppState;

/// Half the cores, 2 to 4: the user is probably still working on this machine.
fn threads() -> i32 {
    let cores = std::thread::available_parallelism().map_or(4, |cores| cores.get());
    (cores / 2).clamp(2, 4) as i32
}

/// The installed high-accuracy model, with the glossary's terms as hotwords.
fn accurate(state: &AppState) -> CommandResult<Model> {
    let missing = || CommandError::new("model-missing", "the high-accuracy model is not installed");
    let manifest = Manifest::builtin();
    let model = manifest.by_role(Role::Accurate).ok_or_else(missing)?;
    let dir = crate::downloads::installed(state, model).ok_or_else(missing)?;
    let kind = model
        .kind
        .as_deref()
        .and_then(ModelKind::parse)
        .ok_or_else(missing)?;
    let mut spec = OfflineModelSpec::new(kind, dir);
    spec.num_threads = threads();
    spec.hotwords = state.glossary().hotwords();
    Ok(Arc::new(SherpaModel(spec)))
}

/// Start a pass over `meeting`; returns the job id.
#[tauri::command]
pub async fn retranscribe(
    app: AppHandle,
    state: State<'_, AppState>,
    meeting: i64,
) -> CommandResult<u64> {
    jobs::idle(&state, meeting)?;
    let engine = state.engine()?.clone();
    if engine
        .state()
        .is_some_and(|(_, session)| session != SessionState::Finalized)
    {
        return Err(CommandError::new("busy", "a recording is in progress"));
    }
    let model = accurate(&state)?;
    jobs::spawn(
        &app,
        meeting,
        Kind::Retranscribe,
        move |out, cancel| async move {
            let (app, job) = (out.app.clone(), out.job);
            let joined = tauri::async_runtime::spawn_blocking(move || {
                let result = engine.retranscribe(
                    meeting,
                    &model,
                    &|| cancel.is_cancelled(),
                    &mut |progress| {
                        out.progress(Stage::Segments {
                            done: progress.done,
                            total: progress.total,
                        });
                    },
                );
                (out, result)
            })
            .await;
            // Some segments may have changed even when the pass stopped early.
            let _ = app.emit("library-changed", ());
            match joined {
                Ok((out, Ok(done))) => {
                    log!(
                        "retranscribe: {} of {} segments changed; {} ms audio, load {} ms, decode {} ms",
                        done.changed,
                        done.total,
                        done.audio_ms,
                        done.load_ms,
                        done.decode_ms
                    );
                    let result = serde_json::json!({
                        "done": done.done,
                        "total": done.total,
                        "changed": done.changed,
                        "audioMs": done.audio_ms,
                        "loadMs": done.load_ms,
                        "decodeMs": done.decode_ms,
                    });
                    (out, Ok(result))
                }
                Ok((out, Err(error))) => (out, Err(error.into())),
                Err(error) => {
                    let out = Out {
                        app,
                        job,
                        meeting,
                        kind: Kind::Retranscribe,
                    };
                    (out, Err(CommandError::new("engine", error.to_string())))
                }
            }
        },
    )
}

/// Put back the text each segment had before the high-accuracy passes, except where the
/// user changed it since. Returns how many segments were restored.
#[tauri::command]
pub async fn revert_retranscription(
    app: AppHandle,
    state: State<'_, AppState>,
    meeting: i64,
) -> CommandResult<usize> {
    let running = state
        .jobs
        .list()
        .iter()
        .any(|(_, id, kind)| *id == meeting && *kind == Kind::Retranscribe);
    if running {
        return Err(CommandError::new("job-running", "this is already running"));
    }
    let restored = state
        .engine()?
        .store()
        .call(move |store| store.revert_retranscription(meeting))?;
    log!("retranscription undone: {restored} segments");
    let _ = app.emit("library-changed", ());
    Ok(restored)
}
