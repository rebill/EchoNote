//! Voice activity detection on top of sherpa-onnx (Silero VAD or TEN VAD).

use std::path::PathBuf;

use sherpa_onnx::{SileroVadModelConfig, TenVadModelConfig, VadModelConfig, VoiceActivityDetector};

use crate::SAMPLE_RATE;

#[derive(Debug, thiserror::Error)]
pub enum VadError {
    #[error("VAD model file is missing: {0}")]
    MissingFile(PathBuf),
    #[error("sherpa-onnx rejected the VAD configuration")]
    CreateFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VadKind {
    Silero,
    Ten,
}

#[derive(Clone, Debug)]
pub struct VadSpec {
    pub kind: VadKind,
    pub model: PathBuf,
    pub threshold: f32,
    /// Trailing silence that closes a speech segment, in seconds.
    pub min_silence_s: f32,
    pub min_speech_s: f32,
    /// Speech longer than this is cut into several segments, in seconds.
    pub max_speech_s: f32,
}

impl VadSpec {
    /// Defaults tuned for meeting speech: short pauses stay inside one sentence.
    pub fn meeting(kind: VadKind, model: impl Into<PathBuf>) -> Self {
        Self {
            kind,
            model: model.into(),
            threshold: 0.5,
            min_silence_s: 0.5,
            min_speech_s: 0.25,
            max_speech_s: 10.0,
        }
    }

    /// Samples per VAD inference window.
    pub fn window_size(&self) -> usize {
        match self.kind {
            VadKind::Silero => 512,
            VadKind::Ten => 256,
        }
    }
}

/// A finished speech segment on the input timeline.
pub struct Segment {
    pub start_sample: u64,
    pub samples: Vec<f32>,
}

pub struct Vad {
    detector: VoiceActivityDetector,
    window_size: usize,
}

impl Vad {
    pub fn create(spec: &VadSpec) -> Result<Self, VadError> {
        if !spec.model.is_file() {
            return Err(VadError::MissingFile(spec.model.clone()));
        }
        let model = Some(spec.model.to_string_lossy().into_owned());
        let window_size = spec.window_size();
        let mut config = VadModelConfig {
            sample_rate: SAMPLE_RATE as i32,
            num_threads: 1,
            provider: Some("cpu".into()),
            ..Default::default()
        };
        match spec.kind {
            VadKind::Silero => {
                config.silero_vad = SileroVadModelConfig {
                    model,
                    threshold: spec.threshold,
                    min_silence_duration: spec.min_silence_s,
                    min_speech_duration: spec.min_speech_s,
                    window_size: window_size as i32,
                    max_speech_duration: spec.max_speech_s,
                }
            }
            VadKind::Ten => {
                config.ten_vad = TenVadModelConfig {
                    model,
                    threshold: spec.threshold,
                    min_silence_duration: spec.min_silence_s,
                    min_speech_duration: spec.min_speech_s,
                    window_size: window_size as i32,
                    max_speech_duration: spec.max_speech_s,
                }
            }
        }
        let buffer_s = spec.max_speech_s + 5.0;
        let detector =
            VoiceActivityDetector::create(&config, buffer_s).ok_or(VadError::CreateFailed)?;
        Ok(Self {
            detector,
            window_size,
        })
    }

    pub fn window_size(&self) -> usize {
        self.window_size
    }

    /// Feed exactly one window of samples.
    pub fn accept_window(&self, window: &[f32]) {
        debug_assert_eq!(window.len(), self.window_size);
        self.detector.accept_waveform(window);
    }

    /// Whether the most recent window was classified as speech.
    pub fn in_speech(&self) -> bool {
        self.detector.detected()
    }

    /// Pop the oldest finished segment, if any.
    pub fn pop_segment(&self) -> Option<Segment> {
        if self.detector.is_empty() {
            return None;
        }
        let segment = self.detector.front()?;
        let result = Segment {
            start_sample: segment.start() as u64,
            samples: segment.samples().to_vec(),
        };
        self.detector.pop();
        Some(result)
    }

    /// Close any open segment at end of input.
    pub fn flush(&self) {
        self.detector.flush();
    }
}
