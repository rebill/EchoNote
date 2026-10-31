//! Per-window speech detection. The segmenter owns segment boundaries; a detector only says
//! whether each window is speech, after its own smoothing, and how late that answer is.

use crate::vad::{Vad, VadError, VadSpec};

pub trait SpeechDetector: Send {
    /// Samples per [`SpeechDetector::accept`] call.
    fn window(&self) -> usize;
    /// Feed exactly one window; returns whether the detector is in speech after it.
    fn accept(&mut self, window: &[f32]) -> bool;
    /// When speech is first reported after window end `fed`, it began at `fed - onset_lag()`.
    fn onset_lag(&self) -> usize;
    /// When speech is first reported over after window end `fed`, it ended at
    /// `fed - release_lag()`. Valid for the window just accepted.
    fn release_lag(&self) -> usize;
}

/// Silero or TEN VAD through sherpa-onnx, which keeps its relaxed mode: after `max_speech_s`
/// of buffered speech it ends speech at a 0.1 s dip instead of 0.5 s of silence, so long
/// utterances still split at short pauses. Continuous speech has none, so the segmenter's hard
/// cap remains the backstop. Sherpa's segments are discarded; only where the last one ended is
/// kept, which gives the exact release lag in either mode.
///
/// The onset lag assumes the Silero v4 or TEN window, for which it is exactly
/// `2 * window + min_speech` (measured in M1). Silero v5 overlaps windows by 64 samples,
/// which would make the onset estimate 40 ms late.
pub struct SherpaDetector {
    vad: Vad,
    window: usize,
    onset_lag: usize,
    default_release: usize,
    release: usize,
    /// Samples accepted so far; sherpa reports positions on the same count.
    fed: usize,
    speech_for: usize,
    /// Flush sherpa's segment after this much continuous speech, before its buffer has to
    /// grow. A flush keeps the model state, so detection continues without a gap.
    max_buffered: usize,
}

impl SherpaDetector {
    pub fn new(spec: &VadSpec) -> Result<Self, VadError> {
        let window = spec.window_size();
        let rate = crate::SAMPLE_RATE as f32;
        let onset_lag = 2 * window + (spec.min_speech_s * rate) as usize;
        let default_release = (spec.min_silence_s * rate) as usize;
        Ok(Self {
            vad: Vad::create(spec)?,
            window,
            onset_lag,
            default_release,
            release: default_release,
            fed: 0,
            speech_for: 0,
            max_buffered: ((spec.buffer_s() - 1.0) * rate) as usize,
        })
    }
}

impl SpeechDetector for SherpaDetector {
    fn window(&self) -> usize {
        self.window
    }

    fn accept(&mut self, window: &[f32]) -> bool {
        self.vad.accept_window(window);
        self.fed += window.len();
        self.release = match self.vad.pop_segment_ends() {
            Some(end) => self.fed.saturating_sub(end as usize),
            None => self.default_release,
        };
        let speech = self.vad.in_speech();
        self.speech_for = if speech {
            self.speech_for + window.len()
        } else {
            0
        };
        if self.speech_for >= self.max_buffered {
            self.vad.flush();
            self.vad.pop_segment_ends();
            self.speech_for = 0;
        }
        speech
    }

    fn onset_lag(&self) -> usize {
        self.onset_lag
    }

    fn release_lag(&self) -> usize {
        self.release
    }
}

/// RMS threshold with onset and release debouncing, for tests and synthetic fixtures.
pub struct EnergyDetector {
    window: usize,
    threshold: f32,
    onset_windows: usize,
    release_windows: usize,
    loud: usize,
    quiet: usize,
    speech: bool,
}

impl EnergyDetector {
    pub fn new(
        window: usize,
        threshold: f32,
        onset_windows: usize,
        release_windows: usize,
    ) -> Self {
        Self {
            window,
            threshold,
            onset_windows,
            release_windows,
            loud: 0,
            quiet: 0,
            speech: false,
        }
    }
}

impl SpeechDetector for EnergyDetector {
    fn window(&self) -> usize {
        self.window
    }

    fn accept(&mut self, window: &[f32]) -> bool {
        let rms =
            (window.iter().map(|sample| sample * sample).sum::<f32>() / window.len() as f32).sqrt();
        if rms >= self.threshold {
            self.loud += 1;
            self.quiet = 0;
        } else {
            self.quiet += 1;
            self.loud = 0;
        }
        if !self.speech && self.loud >= self.onset_windows {
            self.speech = true;
        } else if self.speech && self.quiet >= self.release_windows {
            self.speech = false;
        }
        self.speech
    }

    fn onset_lag(&self) -> usize {
        self.onset_windows * self.window
    }

    fn release_lag(&self) -> usize {
        self.release_windows * self.window
    }
}
