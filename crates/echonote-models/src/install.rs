//! Installing a model: each file from the mirrors in turn (a few tries each), the release
//! archive as the last resort, all of it in a staging directory renamed into place at the end.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::cancel::{Cancel, unless_cancelled};
use crate::fetch::{IDLE, Target, fetch, part_path};
use crate::{Model, ModelError, staged_bytes, staging_dir};

/// Tries per mirror before moving on.
const TRIES: u32 = 3;
/// Room left on the disk after a download.
const SPARE: u64 = 256 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Downloading,
    /// Taking the files out of the release archive.
    Extracting,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub stage: Stage,
    /// Bytes of the model's files done (while extracting: written out).
    pub done: u64,
    pub total: u64,
}

pub struct Installer {
    http: reqwest::Client,
    retry_delay: Duration,
}

/// Plain http only to this machine (tests, a local mirror).
fn allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        || (url.scheme() == "http"
            && url.host_str().is_some_and(|host| {
                host == "localhost"
                    || host
                        .trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            }))
}

impl Installer {
    pub fn new() -> Result<Self, ModelError> {
        let redirects = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                attempt.error("too many redirects")
            } else if allowed(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("redirected to plain http")
            }
        });
        let http = reqwest::Client::builder()
            .user_agent(concat!("EchoNote/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(IDLE)
            .redirect(redirects)
            .build()
            .map_err(|error| ModelError::Network(error.to_string()))?;
        Ok(Self {
            http,
            retry_delay: Duration::from_secs(2),
        })
    }

    /// The pause between tries (tests shorten it).
    pub fn with_retry_delay(mut self, delay: Duration) -> Self {
        self.retry_delay = delay;
        self
    }

    /// Install `model` into `target`, continuing an earlier attempt. Returns at once when it
    /// is already complete. Extracting an archive blocks the thread, so run this on a thread
    /// of its own. `target` must be a directory the app owns: an incomplete one is replaced.
    pub async fn install(
        &self,
        model: &Model,
        target: &Path,
        cancel: &Cancel,
        on_progress: &mut (dyn FnMut(Progress) + Send),
    ) -> Result<(), ModelError> {
        if model.is_complete(target) {
            return Ok(());
        }
        let staging = staging_dir(target);
        std::fs::create_dir_all(&staging)?;
        let total = model.size();
        check_space(&staging, total - staged_bytes(model, target))?;
        let mut done = 0;
        let mut pending = Vec::new();
        for file in &model.files {
            let path = staging.join(&file.path);
            if std::fs::metadata(&path).is_ok_and(|meta| meta.len() == file.size) {
                done += file.size;
            } else {
                pending.push(file);
            }
        }
        on_progress(Progress {
            stage: Stage::Downloading,
            done,
            total,
        });
        let mut use_archive = false;
        for file in pending {
            let path = staging.join(&file.path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let urls: Vec<String> = model
                .mirrors
                .iter()
                .map(|mirror| format!("{}/{}", mirror.trim_end_matches('/'), file.path))
                .collect();
            let target = Target {
                url: "",
                path: &path,
                name: &file.path,
                size: file.size,
                sha256: &file.sha256,
            };
            let start = done;
            let mut report = |bytes: u64| {
                on_progress(Progress {
                    stage: Stage::Downloading,
                    done: start + bytes,
                    total,
                })
            };
            let result = self.fetch_any(&urls, target, cancel, &mut report).await;
            match result {
                Ok(()) => done += file.size,
                Err(error) if error.try_elsewhere() && model.archive.is_some() => {
                    use_archive = true;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        if use_archive {
            self.fetch_archive(model, target, &staging, cancel, on_progress)
                .await?;
        }
        finish(model, &staging, target)
    }

    /// One file from the first URL that works.
    async fn fetch_any(
        &self,
        urls: &[String],
        target: Target<'_>,
        cancel: &Cancel,
        on_bytes: &mut (dyn FnMut(u64) + Send),
    ) -> Result<(), ModelError> {
        let mut last = ModelError::Network("no mirror".into());
        for url in urls {
            if !reqwest::Url::parse(url).is_ok_and(|url| allowed(&url)) {
                last = ModelError::Network("only https downloads are allowed".into());
                continue;
            }
            let target = Target { url, ..target };
            for attempt in 0..TRIES {
                if attempt > 0 {
                    unless_cancelled(cancel, tokio::time::sleep(self.retry_delay))
                        .await
                        .ok_or(ModelError::Cancelled)?;
                }
                let resumed =
                    std::fs::metadata(part_path(target.path)).is_ok_and(|meta| meta.len() > 0);
                match fetch(&self.http, &target, cancel, on_bytes).await {
                    Ok(()) => return Ok(()),
                    // Wrong bytes from a clean start, or a 4xx other than a dropped range or a
                    // rate limit: this mirror will not get better.
                    Err(error @ ModelError::Checksum(_)) if !resumed => {
                        last = error;
                        break;
                    }
                    Err(error @ ModelError::Http(400..=499))
                        if !matches!(error, ModelError::Http(408 | 416 | 429)) =>
                    {
                        last = error;
                        break;
                    }
                    Err(error) if error.try_elsewhere() => last = error,
                    Err(error) => return Err(error),
                }
            }
        }
        Err(last)
    }

    /// The files still missing, out of the release archive.
    async fn fetch_archive(
        &self,
        model: &Model,
        target: &Path,
        staging: &Path,
        cancel: &Cancel,
        on_progress: &mut (dyn FnMut(Progress) + Send),
    ) -> Result<(), ModelError> {
        let archive = model.archive.as_ref().expect("checked by the caller");
        let total = model.size();
        let path = staging.join(".archive");
        let have = std::fs::metadata(part_path(&path)).map_or(0, |meta| meta.len());
        check_space(
            staging,
            archive.size.saturating_sub(have) + total - staged_bytes(model, target),
        )?;
        let target = Target {
            url: &archive.url,
            path: &path,
            name: "archive",
            size: archive.size,
            sha256: &archive.sha256,
        };
        // The archive's own bytes, scaled to the model's size for one progress bar.
        let mut report = |bytes: u64| {
            let done = (bytes as u128 * total as u128 / archive.size.max(1) as u128) as u64;
            on_progress(Progress {
                stage: Stage::Downloading,
                done,
                total,
            });
        };
        self.fetch_any(
            std::slice::from_ref(&archive.url),
            target,
            cancel,
            &mut report,
        )
        .await?;
        on_progress(Progress {
            stage: Stage::Extracting,
            done: 0,
            total,
        });
        let result = crate::archive::extract(
            &path,
            model,
            staging,
            &archive.prefix,
            cancel,
            &mut |done| {
                on_progress(Progress {
                    stage: Stage::Extracting,
                    done,
                    total,
                });
            },
        );
        if !matches!(result, Err(ModelError::Cancelled)) {
            let _ = std::fs::remove_file(&path);
        }
        result
    }
}

fn check_space(dir: &Path, needed: u64) -> Result<(), ModelError> {
    match crate::free_bytes(dir) {
        Some(free) if free < needed + SPARE => Err(ModelError::DiskFull {
            needed: needed + SPARE,
            free,
        }),
        _ => Ok(()),
    }
}

/// Move the finished staging directory into place.
fn finish(model: &Model, staging: &Path, target: &Path) -> Result<(), ModelError> {
    if !model.is_complete(staging) {
        return Err(ModelError::Network(
            "files are missing after the download".into(),
        ));
    }
    if target.exists() {
        std::fs::remove_dir_all(target)?;
    }
    std::fs::rename(staging, target)?;
    Ok(())
}

/// Remove an installed model and whatever was downloaded towards it.
pub fn delete(target: &Path) -> std::io::Result<()> {
    for dir in [target.to_path_buf(), staging_dir(target)] {
        match std::fs::remove_dir_all(&dir) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}
