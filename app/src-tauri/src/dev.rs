//! Debug-build switches for testing the app without devices or the user's library. Release
//! builds ignore the environment.
//!
//! - `ECHONOTE_DATA_DIR`: library and recordings directory.
//! - `ECHONOTE_FAKE_INPUT`: a directory with `mic.wav` (and optionally `sys.wav`) played in
//!   real time instead of the microphone and system audio.
//! - `ECHONOTE_SMOKE`: a file path; the UI records the fake input, checks what it rendered and
//!   writes a JSON report there, then the app quits (`scripts/dev/app_smoke.sh`).
//! - `ECHONOTE_EXPORT_DIR`: exports are written here without the save dialog.

use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct Dev {
    pub data_dir: Option<PathBuf>,
    pub fake_input: Option<PathBuf>,
    pub smoke: Option<PathBuf>,
    pub export_dir: Option<PathBuf>,
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
        }
    }
}
