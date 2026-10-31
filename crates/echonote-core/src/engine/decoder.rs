//! What the ASR worker loads and runs: sherpa-onnx models in the app, a deterministic fake in
//! tests and CI gates.

use std::sync::Arc;
use std::time::Duration;

use crate::asr::{ModelKind, OfflineAsr, OfflineModelSpec, Token, Transcript};
use crate::{SAMPLE_RATE, asr::AsrError};

/// A loaded model. Lives on the worker thread only.
pub trait Decoder {
    /// Decode 16 kHz mono samples.
    fn decode(&self, samples: &[f32]) -> Transcript;
}

/// How to load a model.
pub trait DecoderFactory: Send + Sync {
    /// Identity: the worker reuses a loaded model with the same id.
    fn id(&self) -> String;
    /// Model name stored with each segment.
    fn name(&self) -> String;
    /// Whether tokens carry start times (needed for overlapping cuts and crosstalk checks).
    fn has_token_timestamps(&self) -> bool;
    fn load(&self) -> Result<Box<dyn Decoder>, AsrError>;
}

pub type Model = Arc<dyn DecoderFactory>;

/// An offline sherpa-onnx model.
pub struct SherpaModel(pub OfflineModelSpec);

impl Decoder for OfflineAsr {
    fn decode(&self, samples: &[f32]) -> Transcript {
        self.decode_transcript(samples)
    }
}

impl DecoderFactory for SherpaModel {
    fn id(&self) -> String {
        let spec = &self.0;
        format!(
            "{}:{}:{}:{:?}:{}:{:?}",
            spec.kind.name(),
            spec.dir.display(),
            spec.num_threads,
            spec.language,
            spec.use_itn,
            spec.hotwords
        )
    }

    fn name(&self) -> String {
        let dir = self.0.dir.file_name().map(|name| name.to_string_lossy());
        dir.map_or_else(|| self.0.kind.name().to_owned(), |dir| dir.into_owned())
    }

    fn has_token_timestamps(&self) -> bool {
        self.0.kind.has_token_timestamps()
    }

    fn load(&self) -> Result<Box<dyn Decoder>, AsrError> {
        Ok(Box::new(OfflineAsr::load(&self.0)?))
    }
}

impl SherpaModel {
    pub fn new(kind: ModelKind, dir: impl Into<std::path::PathBuf>) -> Self {
        Self(OfflineModelSpec::new(kind, dir))
    }
}

/// Deterministic stand-in for an ASR model: one token per 100 ms block that holds at least
/// 25 ms of sound above `threshold`, named after the octave of its pitch (`a` = 55 Hz, so
/// 220 Hz is `c` and 440 Hz is `d`). A tone and a quieter, delayed copy of it (speaker bleed)
/// produce the same text whatever the block alignment; other tones produce other text. With
/// `cost` it also sleeps that long per second of audio, which models decode time for queue
/// and backlog tests (the fixed decode-cost model of the CI gates).
#[derive(Clone, Debug)]
pub struct FakeModel {
    pub name: String,
    pub threshold: f32,
    pub cost: f64,
    pub load_time: Duration,
    /// Fail to load (tests the load-failure path).
    pub broken: bool,
    /// Capital letters: a second model that hears the same audio differently.
    pub upper: bool,
}

impl Default for FakeModel {
    fn default() -> Self {
        Self {
            name: "fake".into(),
            threshold: 0.02,
            cost: 0.0,
            load_time: Duration::ZERO,
            broken: false,
            upper: false,
        }
    }
}

struct FakeDecoder(FakeModel);

/// Samples per fake token.
pub const FAKE_TOKEN: usize = SAMPLE_RATE as usize / 10;

impl Decoder for FakeDecoder {
    fn decode(&self, samples: &[f32]) -> Transcript {
        if self.0.cost > 0.0 {
            let audio_s = samples.len() as f64 / f64::from(SAMPLE_RATE);
            std::thread::sleep(Duration::from_secs_f64(audio_s * self.0.cost));
        }
        let tokens: Vec<Token> = samples
            .chunks(FAKE_TOKEN)
            .enumerate()
            .filter_map(|(index, block)| {
                let letter = pitch_letter(block, self.0.threshold)?;
                let letter = if self.0.upper {
                    letter.to_ascii_uppercase()
                } else {
                    letter
                };
                Some(Token {
                    text: format!("\u{2581}{letter}"),
                    start_s: (index * FAKE_TOKEN) as f32 / SAMPLE_RATE as f32,
                })
            })
            .collect();
        let text = tokens
            .iter()
            .map(|token| token.text.replace('\u{2581}', " "))
            .collect::<String>()
            .trim()
            .to_owned();
        Transcript { text, tokens }
    }
}

/// The octave letter of the sound in `block`, measured over the part above `threshold` only,
/// or `None` when that part is shorter than a quarter of the block.
fn pitch_letter(block: &[f32], threshold: f32) -> Option<char> {
    let first = block.iter().position(|x| x.abs() >= threshold)?;
    let last = block.iter().rposition(|x| x.abs() >= threshold)?;
    let span = &block[first..=last];
    if span.len() < block.len() / 4 {
        return None;
    }
    let crossings = span
        .windows(2)
        .filter(|pair| (pair[0] >= 0.0) != (pair[1] >= 0.0))
        .count();
    let hz = crossings as f64 * f64::from(SAMPLE_RATE) / (2.0 * span.len() as f64);
    if hz < 20.0 {
        return None;
    }
    let octave = (hz / 55.0).log2().round().clamp(0.0, 25.0) as u8;
    Some((b'a' + octave) as char)
}

impl DecoderFactory for FakeModel {
    fn id(&self) -> String {
        format!("fake:{}", self.name)
    }

    fn name(&self) -> String {
        self.name.clone()
    }

    fn has_token_timestamps(&self) -> bool {
        true
    }

    fn load(&self) -> Result<Box<dyn Decoder>, AsrError> {
        std::thread::sleep(self.load_time);
        if self.broken {
            return Err(AsrError::CreateFailed("fake"));
        }
        Ok(Box::new(FakeDecoder(self.clone())))
    }
}
