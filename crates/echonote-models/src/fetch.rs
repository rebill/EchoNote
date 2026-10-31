//! One file, resumable: bytes go to `<file>.part`, a retry asks for the rest with `Range`,
//! and the file is renamed into place only when its size and SHA-256 match.

use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};

use crate::ModelError;
use crate::cancel::{Cancel, unless_cancelled};

/// Longest wait for the next bytes.
pub(crate) const IDLE: Duration = Duration::from_secs(60);

pub(crate) fn part_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

#[derive(Clone, Copy)]
pub(crate) struct Target<'a> {
    pub url: &'a str,
    /// Where the finished file goes.
    pub path: &'a Path,
    /// For errors: the file's name in the manifest.
    pub name: &'a str,
    pub size: u64,
    pub sha256: &'a str,
}

fn network(error: reqwest::Error) -> ModelError {
    ModelError::Network(error.without_url().to_string())
}

/// Hash what an earlier attempt left in the part file; returns its length.
fn hash_existing(file: &mut std::fs::File, hasher: &mut Sha256) -> std::io::Result<u64> {
    file.rewind()?;
    let mut buffer = vec![0u8; 1 << 20];
    let mut total = 0;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(total);
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
}

/// The `Content-Range` start of a 206 answer.
fn range_start(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?
        .strip_prefix("bytes ")?
        .split('-')
        .next()?
        .parse()
        .ok()
}

/// Download `target`, continuing a part file left by an earlier attempt. `on_bytes` gets
/// how much of this file is done.
pub(crate) async fn fetch(
    http: &reqwest::Client,
    target: &Target<'_>,
    cancel: &Cancel,
    on_bytes: &mut (dyn FnMut(u64) + Send),
) -> Result<(), ModelError> {
    let part = part_path(target.path);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&part)?;
    let mut hasher = Sha256::new();
    let mut have = hash_existing(&mut file, &mut hasher)?;
    if have > target.size {
        file.set_len(0)?;
        (have, hasher) = (0, Sha256::new());
    }
    on_bytes(have);
    if have < target.size {
        let mut request = http.get(target.url);
        if have > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let response = unless_cancelled(cancel, request.send())
            .await
            .ok_or(ModelError::Cancelled)?
            .map_err(network)?;
        match response.status().as_u16() {
            206 if range_start(&response) == Some(have) => {}
            206 => {
                file.set_len(0)?;
                return Err(ModelError::Network(
                    "the server resumed at another place".into(),
                ));
            }
            // The server sends the whole file again.
            200 => {
                if have > 0 {
                    file.set_len(0)?;
                    (have, hasher) = (0, Sha256::new());
                    on_bytes(0);
                }
            }
            416 => {
                file.set_len(0)?;
                return Err(ModelError::Http(416));
            }
            status => return Err(ModelError::Http(status)),
        }
        let mut body = response.bytes_stream();
        loop {
            let next = unless_cancelled(cancel, tokio::time::timeout(IDLE, body.next()))
                .await
                .ok_or(ModelError::Cancelled)?;
            let chunk = match next {
                Err(_) => return Err(ModelError::Network("the server stopped sending".into())),
                Ok(None) => break,
                Ok(Some(chunk)) => chunk.map_err(network)?,
            };
            if have + chunk.len() as u64 > target.size {
                drop(file);
                let _ = std::fs::remove_file(&part);
                return Err(ModelError::Checksum(target.name.to_owned()));
            }
            file.write_all(&chunk)?;
            hasher.update(&chunk);
            have += chunk.len() as u64;
            on_bytes(have);
        }
        file.flush()?;
        if have < target.size {
            return Err(ModelError::Network("the download stopped early".into()));
        }
    }
    drop(file);
    if format!("{:x}", hasher.finalize()) != target.sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(ModelError::Checksum(target.name.to_owned()));
    }
    std::fs::rename(&part, target.path)?;
    Ok(())
}
