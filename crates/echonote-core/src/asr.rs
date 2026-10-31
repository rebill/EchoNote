//! ASR model loading on top of sherpa-onnx.
//!
//! Model directories follow the layout of the upstream sherpa-onnx release archives, so a
//! [`ModelKind`] plus a directory is enough to build a recognizer.

use std::path::{Path, PathBuf};

use sherpa_onnx::{
    OfflineFunASRNanoModelConfig, OfflineParaformerModelConfig, OfflineQwen3ASRModelConfig,
    OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig, OnlineRecognizer,
    OnlineRecognizerConfig, OnlineStream, OnlineTransducerModelConfig,
};

use crate::SAMPLE_RATE;

#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error("model file is missing: {0}")]
    MissingFile(PathBuf),
    #[error("sherpa-onnx rejected the {0} model configuration")]
    CreateFailed(&'static str),
    #[error("path is not valid UTF-8: {0}")]
    NonUtf8Path(PathBuf),
}

/// Offline (whole-segment) model families evaluated for EchoNote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelKind {
    SenseVoice,
    Paraformer,
    Qwen3Asr,
    FunAsrNano,
}

impl ModelKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::SenseVoice => "sense-voice",
            Self::Paraformer => "paraformer",
            Self::Qwen3Asr => "qwen3-asr",
            Self::FunAsrNano => "funasr-nano",
        }
    }

    /// Whether sherpa-onnx returns per-token start times for this model (checked in M0 for
    /// SenseVoice and Paraformer; unknown models are treated as without).
    pub fn has_token_timestamps(self) -> bool {
        matches!(self, Self::SenseVoice)
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::SenseVoice,
            Self::Paraformer,
            Self::Qwen3Asr,
            Self::FunAsrNano,
        ]
        .into_iter()
        .find(|kind| kind.name() == value)
    }
}

/// Everything needed to build an offline recognizer.
#[derive(Clone, Debug)]
pub struct OfflineModelSpec {
    pub kind: ModelKind,
    pub dir: PathBuf,
    pub num_threads: i32,
    /// Language hint. `None` lets the model detect the language.
    pub language: Option<String>,
    /// Inverse text normalization (numbers, dates) where the model supports it.
    pub use_itn: bool,
    /// Comma-separated hotwords for models that support contextual biasing.
    pub hotwords: Option<String>,
}

impl OfflineModelSpec {
    pub fn new(kind: ModelKind, dir: impl Into<PathBuf>) -> Self {
        Self {
            kind,
            dir: dir.into(),
            num_threads: 2,
            language: None,
            use_itn: true,
            hotwords: None,
        }
    }
}

/// A loaded offline recognizer. Decoding is synchronous and runs on the caller's thread.
pub struct OfflineAsr {
    kind: ModelKind,
    recognizer: OfflineRecognizer,
}

impl OfflineAsr {
    pub fn load(spec: &OfflineModelSpec) -> Result<Self, AsrError> {
        let config = offline_config(spec)?;
        let recognizer =
            OfflineRecognizer::create(&config).ok_or(AsrError::CreateFailed(spec.kind.name()))?;
        Ok(Self {
            kind: spec.kind,
            recognizer,
        })
    }

    pub fn kind(&self) -> ModelKind {
        self.kind
    }

    /// Decode one segment of 16 kHz mono samples and return the recognized text.
    pub fn decode(&self, samples: &[f32]) -> String {
        self.decode_transcript(samples).text
    }

    /// Decode one segment and keep per-token start times where the model provides them.
    pub fn decode_transcript(&self, samples: &[f32]) -> Transcript {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE as i32, samples);
        self.recognizer.decode(&stream);
        let Some(result) = stream.get_result() else {
            return Transcript::default();
        };
        let tokens = match &result.timestamps {
            Some(starts) if starts.len() == result.tokens.len() => result
                .tokens
                .iter()
                .zip(starts)
                .map(|(text, start)| Token {
                    text: text.clone(),
                    start_s: *start,
                })
                .collect(),
            _ => Vec::new(),
        };
        Transcript {
            text: result.text.trim().to_owned(),
            tokens,
        }
    }
}

/// One recognized token and its start time in seconds from the start of the segment.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub text: String,
    pub start_s: f32,
}

/// Recognized text plus timed tokens. `tokens` is empty when the model has no timestamps.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcript {
    pub text: String,
    pub tokens: Vec<Token>,
}

fn offline_config(spec: &OfflineModelSpec) -> Result<OfflineRecognizerConfig, AsrError> {
    let mut config = OfflineRecognizerConfig::default();
    let model = &mut config.model_config;
    model.num_threads = spec.num_threads;
    model.provider = Some("cpu".into());
    match spec.kind {
        ModelKind::SenseVoice => {
            model.sense_voice = OfflineSenseVoiceModelConfig {
                model: Some(file(&spec.dir, "model.int8.onnx")?),
                language: Some(spec.language.clone().unwrap_or_else(|| "auto".into())),
                use_itn: spec.use_itn,
            };
            model.tokens = Some(file(&spec.dir, "tokens.txt")?);
        }
        ModelKind::Paraformer => {
            model.paraformer = OfflineParaformerModelConfig {
                model: Some(file(&spec.dir, "model.int8.onnx")?),
            };
            model.tokens = Some(file(&spec.dir, "tokens.txt")?);
        }
        ModelKind::Qwen3Asr => {
            model.qwen3_asr = OfflineQwen3ASRModelConfig {
                conv_frontend: Some(file(&spec.dir, "conv_frontend.onnx")?),
                encoder: Some(file(&spec.dir, "encoder.int8.onnx")?),
                decoder: Some(file(&spec.dir, "decoder.int8.onnx")?),
                tokenizer: Some(dir(&spec.dir, "tokenizer")?),
                hotwords: spec.hotwords.clone(),
                ..Default::default()
            };
            model.tokens = Some(String::new());
        }
        ModelKind::FunAsrNano => {
            model.funasr_nano = OfflineFunASRNanoModelConfig {
                encoder_adaptor: Some(file(&spec.dir, "encoder_adaptor.int8.onnx")?),
                embedding: Some(file(&spec.dir, "embedding.int8.onnx")?),
                llm: Some(file(&spec.dir, "llm.int8.onnx")?),
                tokenizer: Some(dir(&spec.dir, "Qwen3-0.6B")?),
                language: spec.language.clone(),
                itn: spec.use_itn as i32,
                hotwords: spec.hotwords.clone(),
                ..Default::default()
            };
            model.tokens = Some(String::new());
            config.decoding_method = Some("greedy_search".into());
        }
    }
    Ok(config)
}

/// A streaming transducer used for low-latency previews.
pub struct StreamingAsr {
    recognizer: OnlineRecognizer,
}

impl StreamingAsr {
    /// Load the zipformer transducer layout used by the sherpa-onnx bilingual zh-en release.
    pub fn load_zipformer(model_dir: &Path, num_threads: i32) -> Result<Self, AsrError> {
        let mut config = OnlineRecognizerConfig::default();
        config.model_config.transducer = OnlineTransducerModelConfig {
            encoder: Some(file(model_dir, "encoder-epoch-99-avg-1.int8.onnx")?),
            decoder: Some(file(model_dir, "decoder-epoch-99-avg-1.onnx")?),
            joiner: Some(file(model_dir, "joiner-epoch-99-avg-1.int8.onnx")?),
        };
        config.model_config.tokens = Some(file(model_dir, "tokens.txt")?);
        config.model_config.num_threads = num_threads;
        config.model_config.provider = Some("cpu".into());
        config.decoding_method = Some("greedy_search".into());
        let recognizer = OnlineRecognizer::create(&config)
            .ok_or(AsrError::CreateFailed("streaming-zipformer"))?;
        Ok(Self { recognizer })
    }

    pub fn create_stream(&self) -> OnlineStream {
        self.recognizer.create_stream()
    }

    /// Feed samples and decode every ready frame; returns the current partial text.
    pub fn accept(&self, stream: &OnlineStream, samples: &[f32]) -> String {
        stream.accept_waveform(SAMPLE_RATE as i32, samples);
        while self.recognizer.is_ready(stream) {
            self.recognizer.decode(stream);
        }
        self.recognizer
            .get_result(stream)
            .map(|result| result.text)
            .unwrap_or_default()
    }

    pub fn reset(&self, stream: &OnlineStream) {
        self.recognizer.reset(stream);
    }
}

fn file(base: &Path, name: &str) -> Result<String, AsrError> {
    let path = base.join(name);
    if !path.is_file() {
        return Err(AsrError::MissingFile(path));
    }
    path_string(path)
}

fn dir(base: &Path, name: &str) -> Result<String, AsrError> {
    let path = base.join(name);
    if !path.is_dir() {
        return Err(AsrError::MissingFile(path));
    }
    path_string(path)
}

fn path_string(path: PathBuf) -> Result<String, AsrError> {
    path.into_os_string()
        .into_string()
        .map_err(|raw| AsrError::NonUtf8Path(PathBuf::from(raw)))
}
