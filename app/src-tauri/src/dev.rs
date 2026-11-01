//! Debug-build switches for testing the app without devices or the user's library. Release
//! builds ignore the environment.
//!
//! - `ECHONOTE_DATA_DIR`: library and recordings directory.
//! - `ECHONOTE_FAKE_INPUT`: a directory with `mic.wav` (and optionally `sys.wav`) played in
//!   real time instead of the microphone and system audio.
//! - `ECHONOTE_SMOKE`: a file path; the UI records the fake input, checks what it rendered and
//!   writes a JSON report there, then the app quits (`scripts/dev/app_smoke.sh`).
//! - `ECHONOTE_EXPORT_DIR`: exports are written here without the save dialog.
//! - `ECHONOTE_HIGHLIGHTS_SECONDS`: live highlights interval instead of three minutes.
//! - `ECHONOTE_SMOKE_LLM`: base URL of the smoke run's mock LLM server, for the UI.
//! - `ECHONOTE_MODEL_MIRROR`: base URL that replaces every model's mirrors and archive (a
//!   local server, to test downloads).
//! - `ECHONOTE_SOAK_SECONDS`: with `ECHONOTE_SMOKE`, a soak run instead: the fake input loops,
//!   the UI scrolls the live list and stops after this long (`scripts/dev/soak.sh`).

use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct Dev {
    pub data_dir: Option<PathBuf>,
    pub fake_input: Option<PathBuf>,
    pub smoke: Option<PathBuf>,
    pub export_dir: Option<PathBuf>,
    pub highlights_interval: Option<Duration>,
    pub smoke_llm: Option<String>,
    pub model_mirror: Option<String>,
    pub soak: Option<Duration>,
}

impl Dev {
    pub fn from_env() -> Self {
        if !cfg!(debug_assertions) {
            return Self::default();
        }
        let path = |name: &str| std::env::var_os(name).map(PathBuf::from);
        Self {
            data_dir: path("ECHONOTE_DATA_DIR"),
            fake_input: path("ECHONOTE_FAKE_INPUT"),
            smoke: path("ECHONOTE_SMOKE"),
            export_dir: path("ECHONOTE_EXPORT_DIR"),
            highlights_interval: std::env::var("ECHONOTE_HIGHLIGHTS_SECONDS")
                .ok()
                .and_then(|seconds| seconds.parse().ok())
                .map(Duration::from_secs),
            smoke_llm: std::env::var("ECHONOTE_SMOKE_LLM").ok(),
            model_mirror: std::env::var("ECHONOTE_MODEL_MIRROR").ok(),
            soak: std::env::var("ECHONOTE_SOAK_SECONDS")
                .ok()
                .and_then(|seconds| seconds.parse().ok())
                .map(Duration::from_secs),
        }
    }
}
