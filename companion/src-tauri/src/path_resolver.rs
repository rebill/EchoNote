use std::path::{Path, PathBuf};

pub(crate) fn expand_tilde(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home_dir) = std::env::var_os("HOME") {
            return PathBuf::from(home_dir).join(rest);
        }
    }

    PathBuf::from(value)
}

pub(crate) fn resolve_asr_service_dir(value: &str) -> Option<PathBuf> {
    service_dir_candidates(value)
        .into_iter()
        .find(|candidate| valid_asr_service_dir(candidate))
        .map(|candidate| canonical_or_original(&candidate))
}

pub(crate) fn resolve_existing_dir_result(value: &str) -> Result<PathBuf, String> {
    service_dir_candidates(value)
        .into_iter()
        .find(|candidate| candidate.is_dir())
        .map(|candidate| canonical_or_original(&candidate))
        .ok_or_else(|| format!("ASR service path does not exist or is not a directory: {value}"))
}

pub(crate) fn valid_asr_service_dir(path: &Path) -> bool {
    path.is_dir() && path.join("pyproject.toml").is_file() && path.join("echonote_asr").is_dir()
}

fn service_dir_candidates(value: &str) -> Vec<PathBuf> {
    let path = expand_tilde(value);
    if path.is_absolute() {
        return vec![path];
    }

    let mut candidates = Vec::new();
    for root in path_roots() {
        push_ancestor_candidates(&mut candidates, &root, &path);
    }
    push_source_tree_candidates(&mut candidates);
    push_macos_resource_candidates(&mut candidates);
    candidates
}

fn path_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(current_dir) = std::env::current_dir() {
        push_unique_path(&mut roots, current_dir);
    }
    if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
        push_unique_path(&mut roots, PathBuf::from(manifest_dir));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            push_unique_path(&mut roots, parent.to_path_buf());
        }
    }
    roots
}

fn push_ancestor_candidates(candidates: &mut Vec<PathBuf>, root: &Path, path: &Path) {
    for ancestor in root.ancestors() {
        push_unique_path(candidates, ancestor.join(path));
    }
}

fn push_source_tree_candidates(candidates: &mut Vec<PathBuf>) {
    let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") else {
        return;
    };
    let manifest_dir = PathBuf::from(manifest_dir);
    push_unique_path(candidates, manifest_dir.join("../../asr-service"));
    if let Some(companion_dir) = manifest_dir.parent() {
        if let Some(repo_root) = companion_dir.parent() {
            push_unique_path(candidates, repo_root.join("asr-service"));
        }
    }
}

fn push_macos_resource_candidates(candidates: &mut Vec<PathBuf>) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(exe_dir) = exe.parent() else {
        return;
    };
    let Some(contents_dir) = exe_dir.parent().filter(|_| exe_dir.ends_with("MacOS")) else {
        return;
    };
    push_unique_path(candidates, contents_dir.join("Resources/asr-service"));
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_asr_service_dir, valid_asr_service_dir};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolves_absolute_service_directory() {
        let path = temp_service_dir();

        assert_eq!(
            resolve_asr_service_dir(&path.to_string_lossy()),
            Some(path.canonicalize().expect("canonical temp path"))
        );

        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn rejects_directories_without_asr_service_shape() {
        let path = std::env::temp_dir().join(format!("echonote-path-test-{}", nonce()));
        fs::create_dir_all(&path).expect("create temp directory");

        assert!(!valid_asr_service_dir(&path));
        assert_eq!(resolve_asr_service_dir(&path.to_string_lossy()), None);

        let _ = fs::remove_dir_all(path);
    }

    fn temp_service_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("echonote-path-test-{}", nonce()));
        fs::create_dir_all(dir.join("echonote_asr")).expect("create fake package");
        fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"echonote-asr\"\n",
        )
        .expect("write pyproject");
        dir
    }

    fn nonce() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos()
    }
}
