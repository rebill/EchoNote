//! The model manager. The manifest (`manifest.json`, built in) lists each model's files with
//! size and SHA-256 and where to get them: file mirrors first (ModelScope, then Hugging Face),
//! then the upstream release archive. Downloads resume after a failure or restart, every file
//! is checked before it counts, and a model appears in its final directory only when all of
//! its files are there (a staging directory is renamed at the end).

mod archive;
mod cancel;
mod disk;
mod fetch;
mod install;

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub use cancel::Cancel;
pub use disk::free_bytes;
pub use install::{Installer, Progress, Stage, delete};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The live model (shipped with the app; downloadable to repair it).
    Live,
    /// Voice activity detection (shipped with the app).
    Vad,
    /// The high-accuracy model for transcribing a meeting again.
    Accurate,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct File {
    /// Relative to the model directory, `/`-separated.
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Archive {
    pub url: String,
    pub size: u64,
    pub sha256: String,
    /// The directory the files are under inside the archive.
    pub prefix: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Model {
    pub id: String,
    pub role: Role,
    /// Directory name under `models/`.
    pub dir: String,
    /// The engine's model kind (`sense-voice`, `qwen3-asr`…).
    #[serde(default)]
    pub kind: Option<String>,
    pub license: String,
    /// Peak memory while it runs, for the settings page.
    #[serde(default)]
    pub memory_mb: u32,
    pub files: Vec<File>,
    /// Base URLs; a file's URL is `{mirror}/{path}`.
    pub mirrors: Vec<String>,
    #[serde(default)]
    pub archive: Option<Archive>,
}

impl Model {
    pub fn size(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }

    /// Every file is present with its size. Files are hashed when installed, and a model
    /// directory only appears once all of them passed.
    pub fn is_complete(&self, dir: &Path) -> bool {
        self.files.iter().all(|file| {
            std::fs::metadata(dir.join(&file.path))
                .is_ok_and(|meta| meta.is_file() && meta.len() == file.size)
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub models: Vec<Model>,
}

impl Manifest {
    /// The manifest built into the app.
    pub fn builtin() -> Self {
        serde_json::from_str(include_str!("../manifest.json"))
            .expect("the built-in manifest parses")
    }

    pub fn model(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.id == id)
    }

    pub fn by_role(&self, role: Role) -> Option<&Model> {
        self.models.iter().find(|model| model.role == role)
    }
}

/// Where a model is staged while it downloads: next to its final directory.
pub fn staging_dir(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!(".{name}.partial"))
}

/// Bytes already downloaded for a model that is not installed yet.
pub fn staged_bytes(model: &Model, target: &Path) -> u64 {
    let staging = staging_dir(target);
    model
        .files
        .iter()
        .map(|file| {
            let done = staging.join(&file.path);
            let part = fetch::part_path(&done);
            std::fs::metadata(&done)
                .or_else(|_| std::fs::metadata(part))
                .map_or(0, |meta| meta.len().min(file.size))
        })
        .sum()
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("not enough disk space: {needed} bytes needed, {free} free")]
    DiskFull { needed: u64, free: u64 },
    #[error("the download failed: {0}")]
    Network(String),
    #[error("the server answered {0}")]
    Http(u16),
    #[error("{0} did not match its checksum")]
    Checksum(String),
    #[error("the archive has no {0}")]
    MissingInArchive(String),
    /// A write failed for want of space (the free-space check before a download can be
    /// outrun by other programs).
    #[error("the disk is full")]
    StorageFull,
    #[error("cancelled")]
    Cancelled,
    #[error(transparent)]
    Io(std::io::Error),
}

impl From<std::io::Error> for ModelError {
    fn from(error: std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::StorageFull => Self::StorageFull,
            _ => Self::Io(error),
        }
    }
}

impl ModelError {
    /// A fixed name for logs and the UI's translations.
    pub fn code(&self) -> &'static str {
        match self {
            Self::DiskFull { .. } | Self::StorageFull => "model-disk-full",
            Self::Network(_) => "model-network",
            Self::Http(_) => "model-http",
            Self::Checksum(_) | Self::MissingInArchive(_) => "model-checksum",
            Self::Cancelled => "cancelled",
            Self::Io(_) => "io",
        }
    }

    /// Another mirror might do better.
    fn try_elsewhere(&self) -> bool {
        matches!(
            self,
            Self::Network(_) | Self::Http(_) | Self::Checksum(_) | Self::MissingInArchive(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_manifest_is_consistent() {
        let manifest = Manifest::builtin();
        assert!(manifest.by_role(Role::Live).is_some());
        assert!(manifest.by_role(Role::Vad).is_some());
        let accurate = manifest.by_role(Role::Accurate).expect("an accurate model");
        assert_eq!(accurate.kind.as_deref(), Some("qwen3-asr"));
        for model in &manifest.models {
            assert!(
                !model.mirrors.is_empty() || model.archive.is_some(),
                "{}",
                model.id
            );
            for file in &model.files {
                assert_eq!(file.sha256.len(), 64, "{}", file.path);
                assert!(
                    !file.path.starts_with('/') && !file.path.contains(".."),
                    "{}",
                    file.path
                );
            }
            for url in model
                .mirrors
                .iter()
                .chain(model.archive.iter().map(|a| &a.url))
            {
                assert!(url.starts_with("https://"), "{url}");
            }
        }
    }

    /// The manifest's files are the ones the benchmarks measured.
    #[test]
    fn the_manifest_matches_the_benchmark_assets() {
        let assets: serde_json::Value =
            serde_json::from_str(include_str!("../../echonote-bench/assets.json")).unwrap();
        let known: Vec<(String, u64, String)> = assets["assets"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|asset| asset["files"].as_array().unwrap())
            .filter_map(|file| {
                Some((
                    file["path"].as_str()?.to_owned(),
                    file["size"].as_u64()?,
                    file["sha256"].as_str()?.to_owned(),
                ))
            })
            .collect();
        for model in &Manifest::builtin().models {
            for file in &model.files {
                let entry = (file.path.clone(), file.size, file.sha256.clone());
                assert!(known.contains(&entry), "{} {}", model.id, file.path);
            }
        }
    }

    #[test]
    fn a_full_disk_is_named_as_such() {
        let full = ModelError::from(std::io::Error::from(std::io::ErrorKind::StorageFull));
        assert_eq!(full.code(), "model-disk-full");
        let other = ModelError::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert_eq!(other.code(), "io");
    }

    #[test]
    fn staging_sits_next_to_the_target() {
        assert_eq!(
            staging_dir(Path::new("/m/qwen")),
            PathBuf::from("/m/.qwen.partial")
        );
    }
}
