//! Where the live model and the VAD come from: the user's data directory (installed by the
//! model manager), the app bundle, and in debug builds `$ECHONOTE_DEV_DATA/models`
//! (`scripts/dev/fetch_assets.py`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use echonote_core::asr::ModelKind;
use echonote_core::engine::{EngineConfig, Model, SherpaModel};
use echonote_core::vad::{VadKind, VadSpec};

/// Directory name of the default live model under `models/`.
pub const LIVE_MODEL: &str = "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09";
/// VAD model path under `models/`.
pub const VAD_MODEL: &str = "vad/silero_vad.onnx";

#[derive(Clone, Debug, PartialEq)]
pub struct Located {
    /// The live model directory, if found anywhere.
    pub asr: Option<PathBuf>,
    pub vad: Option<PathBuf>,
}

/// Search `roots` (each a `models/` directory) in order.
pub fn locate(roots: &[PathBuf]) -> Located {
    let asr = roots
        .iter()
        .map(|root| root.join(LIVE_MODEL))
        .find(|dir| dir.join("model.int8.onnx").is_file() && dir.join("tokens.txt").is_file());
    let vad = roots
        .iter()
        .map(|root| root.join(VAD_MODEL))
        .find(|path| path.is_file());
    Located { asr, vad }
}

/// The `models/` directories to search: data directory, bundled resources, dev data.
pub fn roots(data_dir: &Path, resource_dir: Option<PathBuf>) -> Vec<PathBuf> {
    let mut roots = vec![data_dir.join("models")];
    if let Some(resources) = resource_dir {
        roots.push(resources.join("models"));
    }
    if cfg!(debug_assertions) {
        let dev = std::env::var_os("ECHONOTE_DEV_DATA")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache/echonote-dev"))
            });
        if let Some(dev) = dev {
            roots.push(dev.join("models"));
        }
    }
    roots
}

/// Engine configuration for the located models. A missing live model still records: loading
/// fails with a notice and the audio can be transcribed later. A missing VAD fails at start.
pub fn engine_config(data_dir: PathBuf, located: &Located) -> EngineConfig {
    let fallback = data_dir.join("models");
    let asr_dir = located
        .asr
        .clone()
        .unwrap_or_else(|| fallback.join(LIVE_MODEL));
    let model: Model = Arc::new(SherpaModel::new(ModelKind::SenseVoice, asr_dir));
    let vad_path = located
        .vad
        .clone()
        .unwrap_or_else(|| fallback.join(VAD_MODEL));
    let detector = EngineConfig::sherpa_vad(VadSpec::meeting(VadKind::Silero, vad_path));
    EngineConfig::new(data_dir, model, detector)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_root_with_complete_files_wins() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        // `a` has an incomplete model and no VAD; `b` has both.
        std::fs::create_dir_all(a.join(LIVE_MODEL)).unwrap();
        std::fs::write(a.join(LIVE_MODEL).join("tokens.txt"), "").unwrap();
        std::fs::create_dir_all(b.join(LIVE_MODEL)).unwrap();
        std::fs::create_dir_all(b.join("vad")).unwrap();
        for file in ["model.int8.onnx", "tokens.txt"] {
            std::fs::write(b.join(LIVE_MODEL).join(file), "").unwrap();
        }
        std::fs::write(b.join(VAD_MODEL), "").unwrap();
        let found = locate(&[a.clone(), b.clone()]);
        assert_eq!(found.asr, Some(b.join(LIVE_MODEL)));
        assert_eq!(found.vad, Some(b.join(VAD_MODEL)));
        assert_eq!(
            locate(&[a]),
            Located {
                asr: None,
                vad: None
            }
        );
    }
}
