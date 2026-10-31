//! The manifest's files out of an upstream `.tar.bz2`, each checked like a download. Blocks
//! the calling thread (decompressing takes a while).

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::cancel::Cancel;
use crate::fetch::part_path;
use crate::{Model, ModelError};

pub(crate) fn extract(
    archive: &Path,
    model: &Model,
    staging: &Path,
    prefix: &str,
    cancel: &Cancel,
    on_bytes: &mut (dyn FnMut(u64) + Send),
) -> Result<(), ModelError> {
    let reader = std::io::BufReader::new(std::fs::File::open(archive)?);
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(reader));
    // Files the mirrors already delivered (checked) stay.
    let (have, missing): (Vec<_>, Vec<_>) = model.files.iter().partition(|file| {
        std::fs::metadata(staging.join(&file.path)).is_ok_and(|meta| meta.len() == file.size)
    });
    let wanted: HashSet<&str> = missing.iter().map(|file| file.path.as_str()).collect();
    let mut done: u64 = have.iter().map(|file| file.size).sum();
    let mut found = HashSet::new();
    let mut buffer = vec![0u8; 1 << 20];
    for entry in tar.entries()? {
        if cancel.is_cancelled() {
            return Err(ModelError::Cancelled);
        }
        let mut entry = entry?;
        let name = entry.path()?.to_string_lossy().replace('\\', "/");
        let name = name.trim_start_matches("./");
        let Some(rel) = name
            .strip_prefix(prefix)
            .map(|rest| rest.trim_start_matches('/'))
        else {
            continue;
        };
        // The destination comes from the manifest, never from the archive's own path.
        let Some(file) = model
            .files
            .iter()
            .find(|file| file.path == rel && wanted.contains(rel))
        else {
            continue;
        };
        let dest = staging.join(&file.path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let part = part_path(&dest);
        let mut out = std::fs::File::create(&part)?;
        let mut hasher = Sha256::new();
        let mut size = 0u64;
        loop {
            if cancel.is_cancelled() {
                drop(out);
                let _ = std::fs::remove_file(&part);
                return Err(ModelError::Cancelled);
            }
            let read = entry.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            out.write_all(&buffer[..read])?;
            hasher.update(&buffer[..read]);
            size += read as u64;
            on_bytes(done + size);
        }
        out.flush()?;
        drop(out);
        if size != file.size || format!("{:x}", hasher.finalize()) != file.sha256 {
            let _ = std::fs::remove_file(&part);
            return Err(ModelError::Checksum(file.path.clone()));
        }
        std::fs::rename(&part, &dest)?;
        done += size;
        found.insert(rel.to_owned());
        if found.len() == wanted.len() {
            // The rest of the archive (sample audio) is not needed.
            break;
        }
    }
    match wanted.iter().find(|path| !found.contains(**path)) {
        Some(missing) => Err(ModelError::MissingInArchive((*missing).to_owned())),
        None => Ok(()),
    }
}
