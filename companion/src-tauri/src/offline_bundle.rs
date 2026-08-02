use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const OFFLINE_MANIFEST_NAME: &str = "echonote-offline-manifest.json";
const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineBundleManifest {
    pub schema_version: u32,
    pub bundle_version: String,
    pub platform: String,
    pub python_version: String,
    pub wheels_path: String,
    pub requirements_path: String,
    pub models: Vec<OfflineModel>,
    pub files: Vec<OfflineFile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineModel {
    pub role: OfflineModelRole,
    pub preset: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OfflineModelRole {
    Asr,
    Diarization,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct VerifiedOfflineBundle {
    pub root: PathBuf,
    pub manifest: OfflineBundleManifest,
    pub wheels_dir: PathBuf,
    pub requirements_file: PathBuf,
    pub asr_model_source: PathBuf,
    pub diarization_model_source: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct InstalledOfflineModels {
    pub asr_model_path: PathBuf,
    pub diarization_model_path: Option<PathBuf>,
}

pub fn probe_bundle(
    root: &Path,
    selected_preset: &str,
    diarization_enabled: bool,
) -> Result<VerifiedOfflineBundle, String> {
    load_bundle(root, selected_preset, diarization_enabled, false)
}

pub fn verify_bundle(
    root: &Path,
    selected_preset: &str,
    diarization_enabled: bool,
) -> Result<VerifiedOfflineBundle, String> {
    load_bundle(root, selected_preset, diarization_enabled, true)
}

fn load_bundle(
    root: &Path,
    selected_preset: &str,
    diarization_enabled: bool,
    verify_hashes: bool,
) -> Result<VerifiedOfflineBundle, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("Offline bundle directory is unavailable: {error}"))?;
    let manifest_path = root.join(OFFLINE_MANIFEST_NAME);
    let manifest_raw = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("Failed to read offline bundle manifest: {error}"))?;
    let manifest: OfflineBundleManifest = serde_json::from_str(&manifest_raw)
        .map_err(|error| format!("Offline bundle manifest is invalid: {error}"))?;

    if manifest.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(format!(
            "Unsupported offline bundle schema version {}.",
            manifest.schema_version
        ));
    }
    if manifest.platform != current_platform() {
        return Err(format!(
            "Offline bundle targets {}, but this system is {}.",
            manifest.platform,
            current_platform()
        ));
    }
    if manifest.bundle_version.trim().is_empty() || manifest.python_version.trim().is_empty() {
        return Err("Offline bundle version and Python version are required.".to_string());
    }

    let wheels_dir = resolve_safe_path(&root, &manifest.wheels_path)?;
    if !wheels_dir.is_dir() {
        return Err(format!(
            "Offline wheel directory was not found: {}",
            wheels_dir.display()
        ));
    }
    let requirements_file = resolve_safe_path(&root, &manifest.requirements_path)?;
    if !requirements_file.is_file() {
        return Err(format!(
            "Offline requirements file was not found: {}",
            requirements_file.display()
        ));
    }

    let asr_model = manifest
        .models
        .iter()
        .find(|model| {
            model.role == OfflineModelRole::Asr && model.preset.as_deref() == Some(selected_preset)
        })
        .ok_or_else(|| format!("Offline bundle does not contain ASR preset {selected_preset}."))?;
    let asr_model_source = resolve_safe_path(&root, &asr_model.path)?;
    validate_model_directory(&asr_model_source, "ASR")?;
    if !asr_model_source.join("config.json").is_file() {
        return Err(format!(
            "Offline ASR model is missing config.json: {}",
            asr_model_source.display()
        ));
    }

    let diarization_model_source = if diarization_enabled {
        let model = manifest
            .models
            .iter()
            .find(|model| model.role == OfflineModelRole::Diarization)
            .ok_or_else(|| "Offline bundle does not contain a diarization model.".to_string())?;
        let path = resolve_safe_path(&root, &model.path)?;
        validate_model_directory(&path, "diarization")?;
        if !path.join("config.yaml").is_file() {
            return Err(format!(
                "Offline diarization model is missing config.yaml: {}",
                path.display()
            ));
        }
        Some(path)
    } else {
        None
    };

    validate_file_manifest(&root, &manifest.files, verify_hashes)?;
    Ok(VerifiedOfflineBundle {
        root,
        manifest,
        wheels_dir,
        requirements_file,
        asr_model_source,
        diarization_model_source,
    })
}

pub fn install_models(
    bundle: &VerifiedOfflineBundle,
    target_root: &Path,
) -> Result<InstalledOfflineModels, String> {
    let target_name = target_root.file_name().ok_or_else(|| {
        format!(
            "Offline model target has no directory name: {}",
            target_root.display()
        )
    })?;
    let parent = target_root.parent().ok_or_else(|| {
        format!(
            "Offline model target has no parent: {}",
            target_root.display()
        )
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create offline model parent directory: {error}"))?;
    let parent = parent
        .canonicalize()
        .map_err(|error| format!("Failed to resolve offline model parent: {error}"))?;
    let target_root = parent.join(target_name);
    if target_root.starts_with(&bundle.root) || bundle.root.starts_with(&target_root) {
        return Err(
            "Offline model destination must be separate from the source bundle.".to_string(),
        );
    }
    if target_root.exists()
        && fs::symlink_metadata(&target_root)
            .map_err(|error| format!("Failed to inspect model destination: {error}"))?
            .file_type()
            .is_symlink()
    {
        return Err(format!(
            "Offline model destination cannot be a symlink: {}",
            target_root.display()
        ));
    }
    let suffix = nonce();
    let staging = parent.join(format!(".echonote-models-staging-{suffix}"));
    let backup = parent.join(format!(".echonote-models-backup-{suffix}"));
    fs::remove_dir_all(&staging).ok();
    fs::remove_dir_all(&backup).ok();
    fs::create_dir_all(&staging)
        .map_err(|error| format!("Failed to create model staging directory: {error}"))?;

    let asr_name = model_directory_name(&bundle.asr_model_source)?;
    let staged_asr = staging.join(&asr_name);
    if let Err(error) = copy_tree(&bundle.asr_model_source, &staged_asr) {
        fs::remove_dir_all(&staging).ok();
        return Err(error);
    }
    let staged_diarization = if let Some(source) = &bundle.diarization_model_source {
        let name = model_directory_name(source)?;
        let destination = staging.join(&name);
        if let Err(error) = copy_tree(source, &destination) {
            fs::remove_dir_all(&staging).ok();
            return Err(error);
        }
        Some((name, destination))
    } else {
        None
    };

    let had_previous = target_root.exists();
    if had_previous {
        fs::rename(&target_root, &backup)
            .map_err(|error| format!("Failed to preserve existing offline models: {error}"))?;
    }
    activate_staging(&staging, &target_root, &backup, had_previous)?;

    Ok(InstalledOfflineModels {
        asr_model_path: target_root.join(asr_name),
        diarization_model_path: staged_diarization.map(|(name, _)| target_root.join(name)),
    })
}

pub fn current_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn validate_file_manifest(
    root: &Path,
    files: &[OfflineFile],
    verify_hashes: bool,
) -> Result<(), String> {
    if files.is_empty() {
        return Err("Offline bundle file manifest is empty.".to_string());
    }
    let mut declared = BTreeSet::new();
    for entry in files {
        let path = resolve_safe_path(root, &entry.path)?;
        let digest = entry.sha256.trim();
        if digest.len() != 64 || !digest.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(format!(
                "Offline bundle contains an invalid SHA-256 digest: {}",
                entry.path
            ));
        }
        if !declared.insert(entry.path.clone()) {
            return Err(format!(
                "Offline bundle manifest contains a duplicate file: {}",
                entry.path
            ));
        }
        if !verify_hashes {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("Offline bundle file is missing ({}): {error}", entry.path))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "Offline bundle entry must be a regular file: {}",
                entry.path
            ));
        }
        if metadata.len() != entry.size {
            return Err(format!("Offline bundle file size mismatch: {}", entry.path));
        }
        let actual = sha256_file(&path)?;
        if !actual.eq_ignore_ascii_case(digest) {
            return Err(format!("Offline bundle SHA-256 mismatch: {}", entry.path));
        }
    }
    if verify_hashes {
        let actual = collect_bundle_files(root)?;
        if actual != declared {
            let missing = declared.difference(&actual).next();
            let undeclared = actual.difference(&declared).next();
            return Err(match (missing, undeclared) {
                (Some(path), _) => format!("Offline bundle file is missing: {path}"),
                (_, Some(path)) => format!("Offline bundle contains an undeclared file: {path}"),
                _ => "Offline bundle file manifest does not match its contents.".to_string(),
            });
        }
    }
    Ok(())
}

fn collect_bundle_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = BTreeSet::new();
    collect_bundle_files_from(root, root, &mut files)?;
    Ok(files)
}

fn collect_bundle_files_from(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<String>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("Failed to inspect offline bundle: {error}"))?
    {
        let entry = entry.map_err(|error| format!("Failed to inspect bundle entry: {error}"))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Offline bundle contains an unsupported symlink: {}",
                path.display()
            ));
        }
        if metadata.is_dir() {
            collect_bundle_files_from(root, &path, files)?;
        } else if metadata.is_file() {
            if path.file_name().and_then(|name| name.to_str()) == Some(OFFLINE_MANIFEST_NAME) {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "Offline bundle path escaped its root.".to_string())?;
            files.insert(relative.to_string_lossy().replace('\\', "/"));
        } else {
            return Err(format!(
                "Offline bundle contains an unsupported file: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn activate_staging(
    staging: &Path,
    target_root: &Path,
    backup: &Path,
    had_previous: bool,
) -> Result<(), String> {
    if let Err(error) = fs::rename(staging, target_root) {
        if had_previous {
            fs::rename(backup, target_root).map_err(|restore_error| {
                format!(
                    "Failed to activate offline models ({error}) and restore previous models ({restore_error})."
                )
            })?;
        }
        fs::remove_dir_all(staging).ok();
        return Err(format!("Failed to activate offline models: {error}"));
    }
    fs::remove_dir_all(backup).ok();
    Ok(())
}

fn validate_model_directory(path: &Path, label: &str) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!(
            "Offline {label} model directory was not found: {}",
            path.display()
        ));
    }
    Ok(())
}

fn resolve_safe_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative_path = Path::new(relative);
    if relative.trim().is_empty()
        || relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "Offline bundle contains an unsafe path: {relative}"
        ));
    }
    Ok(root.join(relative_path))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let file = File::open(path)
        .map_err(|error| format!("Failed to open {} for hashing: {error}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("Failed to hash {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("Failed to create {}: {error}", destination.display()))?;
    for entry in fs::read_dir(source)
        .map_err(|error| format!("Failed to read {}: {error}", source.display()))?
    {
        let entry = entry.map_err(|error| format!("Failed to inspect model entry: {error}"))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| format!("Failed to inspect {}: {error}", source_path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Offline model contains an unsupported symlink: {}",
                source_path.display()
            ));
        }
        if metadata.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)
                .map_err(|error| format!("Failed to copy {}: {error}", source_path.display()))?;
        } else {
            return Err(format!(
                "Offline model contains an unsupported file: {}",
                source_path.display()
            ));
        }
    }
    Ok(())
}

fn model_directory_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "Offline model path has no valid directory name: {}",
                path.display()
            )
        })
}

fn nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{
        activate_staging, current_platform, install_models, verify_bundle, OfflineBundleManifest,
        OfflineFile, OfflineModel, OfflineModelRole, OFFLINE_MANIFEST_NAME,
    };
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn verifies_and_atomically_installs_an_offline_bundle() {
        let root = temp_dir("bundle");
        create_fixture_bundle(&root);
        let verified = verify_bundle(&root, "qwen3-0.6b-4bit", true).expect("verify bundle");
        let target = temp_dir("target-parent").join("models");
        fs::create_dir_all(&target).expect("create old models");
        fs::write(target.join("old.txt"), "old").expect("write old model");

        let installed = install_models(&verified, &target).expect("install models");

        assert!(installed.asr_model_path.join("config.json").is_file());
        assert!(installed
            .diarization_model_path
            .expect("diarization path")
            .join("config.yaml")
            .is_file());
        assert!(!target.join("old.txt").exists());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(target.parent().expect("target parent"));
    }

    #[test]
    fn rejects_a_tampered_bundle_before_installation() {
        let root = temp_dir("tampered-bundle");
        create_fixture_bundle(&root);
        fs::write(
            root.join("models/qwen3-asr-0.6b-4bit/config.json"),
            "tampered",
        )
        .expect("tamper model");

        let error =
            verify_bundle(&root, "qwen3-0.6b-4bit", true).expect_err("reject tampered bundle");

        assert!(error.contains("size mismatch") || error.contains("SHA-256 mismatch"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_manifest_path_traversal() {
        let root = temp_dir("unsafe-bundle");
        create_fixture_bundle(&root);
        let manifest_path = root.join(OFFLINE_MANIFEST_NAME);
        let mut manifest: OfflineBundleManifest =
            serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest"))
                .expect("parse manifest");
        manifest.files[0].path = "../outside".to_string();
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).expect("serialize manifest"),
        )
        .expect("write manifest");

        let error = verify_bundle(&root, "qwen3-0.6b-4bit", true).expect_err("reject traversal");

        assert!(error.contains("unsafe path"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_undeclared_bundle_files() {
        let root = temp_dir("undeclared-bundle");
        create_fixture_bundle(&root);
        fs::write(root.join("wheels/undeclared.whl"), "unexpected").expect("write extra file");

        let error =
            verify_bundle(&root, "qwen3-0.6b-4bit", true).expect_err("reject undeclared file");

        assert!(error.contains("undeclared file"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restores_previous_models_when_activation_fails() {
        let parent = temp_dir("rollback-parent");
        let target = parent.join("models");
        let backup = parent.join("models-backup");
        fs::create_dir_all(&target).expect("create previous models");
        fs::write(target.join("previous.txt"), "previous").expect("write previous model");
        fs::rename(&target, &backup).expect("stage backup");
        let missing_staging = parent.join("missing-staging");

        let error = activate_staging(&missing_staging, &target, &backup, true)
            .expect_err("activation must fail");

        assert!(error.contains("Failed to activate offline models"));
        assert_eq!(
            fs::read_to_string(target.join("previous.txt")).expect("restored model"),
            "previous"
        );
        assert!(!backup.exists());
        let _ = fs::remove_dir_all(parent);
    }

    fn create_fixture_bundle(root: &Path) {
        let files = [
            ("wheels/echonote_asr.whl", b"wheel".as_slice()),
            (
                "requirements-offline.txt",
                b"echonote-asr[mlx,diarization]==0.9.0\n".as_slice(),
            ),
            ("models/qwen3-asr-0.6b-4bit/config.json", b"{}".as_slice()),
            (
                "models/speaker-diarization-community-1/config.yaml",
                b"pipeline: {}".as_slice(),
            ),
        ];
        let mut manifest_files = Vec::new();
        for (relative, content) in files {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().expect("file parent")).expect("create fixture parent");
            fs::write(&path, content).expect("write fixture");
            manifest_files.push(OfflineFile {
                path: relative.to_string(),
                size: content.len() as u64,
                sha256: format!("{:x}", Sha256::digest(content)),
            });
        }
        let manifest = OfflineBundleManifest {
            schema_version: 1,
            bundle_version: "0.9.0-test".to_string(),
            platform: current_platform(),
            python_version: "3.11".to_string(),
            wheels_path: "wheels".to_string(),
            requirements_path: "requirements-offline.txt".to_string(),
            models: vec![
                OfflineModel {
                    role: OfflineModelRole::Asr,
                    preset: Some("qwen3-0.6b-4bit".to_string()),
                    path: "models/qwen3-asr-0.6b-4bit".to_string(),
                },
                OfflineModel {
                    role: OfflineModelRole::Diarization,
                    preset: None,
                    path: "models/speaker-diarization-community-1".to_string(),
                },
            ],
            files: manifest_files,
        };
        fs::write(
            root.join(OFFLINE_MANIFEST_NAME),
            serde_json::to_vec_pretty(&manifest).expect("serialize manifest"),
        )
        .expect("write manifest");
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("echonote-{label}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp directory");
        path
    }
}
