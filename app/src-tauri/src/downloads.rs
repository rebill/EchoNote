//! The model manager behind Settings: what the built-in manifest lists, where each model is
//! installed, and downloads into the data directory with progress on the `models` event.
//! The live model and the VAD ship with the app (a download repairs them); the
//! high-accuracy model is downloaded on request and can be deleted again.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use echonote_models::{Cancel, Installer, Manifest, Model, ModelError, Progress, Role};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::{CommandError, CommandResult};
use crate::log::log;
use crate::state::{AppState, lock};

/// How often download progress is sent to the UI.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    id: String,
    role: &'static str,
    /// Download size (the files, not an archive).
    size: u64,
    memory_mb: u32,
    license: String,
    installed: bool,
    /// Installed in the data directory, where delete can remove it.
    removable: bool,
    /// Bytes kept from an unfinished download.
    partial: u64,
    downloading: bool,
}

#[derive(Clone, Serialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum ModelEvent {
    Progress {
        id: String,
        #[serde(flatten)]
        progress: Progress,
    },
    Done {
        id: String,
    },
    Error {
        id: String,
        error: CommandError,
    },
}

fn role(role: Role) -> &'static str {
    match role {
        Role::Live => "live",
        Role::Vad => "vad",
        Role::Accurate => "accurate",
    }
}

/// Where downloads go: `<data dir>/models/<dir>`.
fn target(state: &AppState, model: &Model) -> PathBuf {
    state.data_dir.join("models").join(&model.dir)
}

/// The directory of a complete copy of `model`, if any root has one.
pub fn installed(state: &AppState, model: &Model) -> Option<PathBuf> {
    state
        .model_roots
        .iter()
        .map(|root| root.join(&model.dir))
        .find(|dir| model.is_complete(dir))
}

fn manifest(state: &AppState) -> Manifest {
    let mut manifest = Manifest::builtin();
    if let Some(mirror) = &state.dev.model_mirror {
        for model in &mut manifest.models {
            model.mirrors = vec![format!("{}/{}", mirror.trim_end_matches('/'), model.dir)];
            model.archive = None;
        }
    }
    manifest
}

fn find(state: &AppState, id: &str) -> CommandResult<Model> {
    manifest(state)
        .model(id)
        .cloned()
        .ok_or_else(|| CommandError::new("not-found", "no such model"))
}

#[tauri::command]
pub async fn asr_models(state: State<'_, AppState>) -> CommandResult<Vec<ModelView>> {
    let downloads = lock(&state.downloads).keys().cloned().collect::<Vec<_>>();
    Ok(manifest(&state)
        .models
        .iter()
        .map(|model| {
            let target = target(&state, model);
            let found = installed(&state, model);
            ModelView {
                id: model.id.clone(),
                role: role(model.role),
                size: model.size(),
                memory_mb: model.memory_mb,
                license: model.license.clone(),
                installed: found.is_some(),
                removable: found.as_ref() == Some(&target),
                partial: echonote_models::staged_bytes(model, &target),
                downloading: downloads.contains(&model.id),
            }
        })
        .collect())
}

/// Start downloading `id`; progress and the outcome come on the `models` event.
#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    let model = find(&state, &id)?;
    let target = target(&state, &model);
    let cancel = Cancel::new();
    {
        let mut downloads = lock(&state.downloads);
        if downloads.contains_key(&id) {
            return Err(CommandError::new("job-running", "the download is running"));
        }
        downloads.insert(id.clone(), cancel.clone());
    }
    let spawned = std::thread::Builder::new()
        .name("model-download".into())
        .spawn(move || {
            let send = |event: ModelEvent| {
                let _ = app.emit("models", event);
            };
            let mut last: Option<(Instant, echonote_models::Stage)> = None;
            let mut on_progress = |progress: Progress| {
                let due = last.is_none_or(|(at, stage)| {
                    at.elapsed() >= PROGRESS_EVERY
                        || stage != progress.stage
                        || progress.done == progress.total
                });
                if due {
                    last = Some((Instant::now(), progress.stage));
                    send(ModelEvent::Progress {
                        id: model.id.clone(),
                        progress,
                    });
                }
            };
            log!("model {} download", model.id);
            let result = Installer::new().and_then(|installer| {
                tauri::async_runtime::block_on(installer.install(
                    &model,
                    &target,
                    &cancel,
                    &mut on_progress,
                ))
            });
            lock(&app.state::<AppState>().downloads).remove(&model.id);
            match result {
                Ok(()) => {
                    log!("model {} installed", model.id);
                    send(ModelEvent::Done {
                        id: model.id.clone(),
                    });
                }
                Err(error) => {
                    log!("model {} download: {}", model.id, error.code());
                    send(ModelEvent::Error {
                        id: model.id.clone(),
                        error: error.into(),
                    });
                }
            }
        });
    if let Err(error) = spawned {
        lock(&state.downloads).remove(&id);
        return Err(CommandError::new("engine", error.to_string()));
    }
    Ok(())
}

/// Stop a download; what arrived is kept for the next try.
#[tauri::command]
pub async fn cancel_download(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    if let Some(cancel) = lock(&state.downloads).get(&id) {
        cancel.cancel();
    }
    Ok(())
}

/// Delete the downloaded high-accuracy model (or what an unfinished download left).
#[tauri::command]
pub async fn delete_model(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let model = find(&state, &id)?;
    if model.role != Role::Accurate {
        return Err(CommandError::new(
            "model-required",
            "this model is needed to record",
        ));
    }
    if lock(&state.downloads).contains_key(&id)
        || state
            .jobs
            .list()
            .iter()
            .any(|(_, _, kind)| *kind == crate::ai::jobs::Kind::Retranscribe)
    {
        return Err(CommandError::new("model-in-use", "the model is in use"));
    }
    echonote_models::delete(&target(&state, &model)).map_err(ModelError::from)?;
    log!("model {} deleted", model.id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app looks for the live model and the VAD where the manifest installs them.
    #[test]
    fn the_manifest_installs_where_the_app_looks() {
        let manifest = Manifest::builtin();
        let live = manifest.by_role(Role::Live).unwrap();
        assert_eq!(live.dir, crate::models::LIVE_MODEL);
        let files: Vec<&str> = live.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(files, ["model.int8.onnx", "tokens.txt"]);
        let vad = manifest.by_role(Role::Vad).unwrap();
        assert_eq!(
            format!("{}/{}", vad.dir, vad.files[0].path),
            crate::models::VAD_MODEL
        );
    }
}
