use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use echonote_core::asr::{ModelKind, OfflineModelSpec};

/// Development data directory shared with `scripts/dev/fetch_assets.py`.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ECHONOTE_DEV_DATA") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .unwrap_or_default();
    PathBuf::from(home).join(".cache").join("echonote-dev")
}

pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}

fn default_model_dir(kind: ModelKind) -> PathBuf {
    let name = match kind {
        ModelKind::SenseVoice => "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09",
        ModelKind::Paraformer => "sherpa-onnx-paraformer-zh-int8-2025-10-07",
        ModelKind::Qwen3Asr => "sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25",
        ModelKind::FunAsrNano => "sherpa-onnx-funasr-nano-int8-2025-12-30",
    };
    models_dir().join(name)
}

pub fn default_streaming_dir() -> PathBuf {
    models_dir().join("sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20")
}

#[derive(clap::Args, Clone, Debug)]
pub struct ModelArgs {
    /// Model family: sense-voice, paraformer, qwen3-asr, funasr-nano.
    #[arg(long, default_value = "sense-voice")]
    pub model: String,
    /// Model directory. Defaults to the layout written by fetch_assets.py.
    #[arg(long)]
    pub model_dir: Option<PathBuf>,
    /// ONNX Runtime intra-op threads.
    #[arg(long, default_value_t = 2)]
    pub threads: i32,
    /// Language hint (for example zh or en). Omit to let the model detect it.
    #[arg(long)]
    pub language: Option<String>,
    /// Disable inverse text normalization; use for references with spelled-out numbers.
    #[arg(long)]
    pub no_itn: bool,
    /// Comma-separated hotwords (Qwen3-ASR and Fun-ASR-Nano only).
    #[arg(long)]
    pub hotwords: Option<String>,
}

impl ModelArgs {
    pub fn spec(&self) -> Result<OfflineModelSpec> {
        let Some(kind) = ModelKind::parse(&self.model) else {
            bail!("unknown model {:?}", self.model);
        };
        let dir = self
            .model_dir
            .clone()
            .unwrap_or_else(|| default_model_dir(kind));
        if !dir.is_dir() {
            return Err(anyhow::anyhow!(
                "model directory {} does not exist",
                dir.display()
            ))
            .context("run scripts/dev/fetch_assets.py first");
        }
        Ok(OfflineModelSpec {
            kind,
            dir,
            num_threads: self.threads,
            language: self.language.clone(),
            use_itn: !self.no_itn,
            hotwords: self.hotwords.clone(),
        })
    }
}
