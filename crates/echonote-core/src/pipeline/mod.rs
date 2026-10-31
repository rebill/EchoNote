//! Live transcription pipeline shared by the app and `echonote-bench`: a segmenter per track
//! and one ASR job queue. It owns no thread and no clock. Callers pass `now_s` on their own
//! clock, so the same code runs in real time and in the bench's virtual-time simulation.
//!
//! Finals always go before previews. Previews are not queued: when the decoder is idle and the
//! shared preview interval has passed, [`Pipeline::next_job`] builds one from the newest audio,
//! so a busy decoder skips previews instead of falling behind. Finals are never dropped; with a
//! priority track its finals go first, since the other track's finals wait for them anyway to
//! rule out crosstalk.

mod detector;
mod segmenter;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;

pub use detector::{EnergyDetector, SherpaDetector, SpeechDetector};
pub use segmenter::{Cut, Region, Segmenter, SegmenterConfig};

use crate::asr::Transcript;

#[derive(Clone, Debug)]
pub struct PipelineConfig {
    pub segmenter: SegmenterConfig,
    /// Re-decode the growing utterance for previews (strategy S1).
    pub previews: bool,
    /// Minimum seconds between previews; all tracks share this one slot. Raised to twice the
    /// last preview decode time.
    pub preview_interval_s: f64,
    /// Minimum utterance length in samples before its first preview.
    pub min_preview: usize,
    /// While this track speaks only it gets previews. Set to the system-audio track: through
    /// speakers the mic then mostly hears the other side again.
    pub priority_track: Option<usize>,
}

impl PipelineConfig {
    pub fn meeting(hard_cap_s: f64, overlap_s: f64) -> Self {
        Self {
            segmenter: SegmenterConfig::meeting(hard_cap_s, overlap_s),
            previews: true,
            preview_interval_s: 0.6,
            min_preview: crate::SAMPLE_RATE as usize * 3 / 10,
            priority_track: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    Final(Cut),
    Preview { utterance: u64, span_start: usize },
}

/// Decode work handed to the worker.
#[derive(Debug)]
pub struct Job {
    pub track: usize,
    pub kind: JobKind,
    /// Timeline index of `samples[0]`.
    pub start: usize,
    /// Empty for a final whose audio was spilled (see [`Pipeline::spill_finals`]): read
    /// `start..start + len` back from the track's recording instead.
    pub samples: Vec<f32>,
    /// Caller time when the job became available.
    pub ready_s: f64,
}

impl Job {
    /// Timeline range of the audio this job decodes.
    pub fn range(&self) -> (usize, usize) {
        match self.kind {
            JobKind::Final(cut) => (cut.audio_start, cut.audio_end),
            JobKind::Preview { .. } => (self.start, self.start + self.samples.len()),
        }
    }

    /// The audio was dropped from memory and has to be read from the recording.
    pub fn is_spilled(&self) -> bool {
        let (start, end) = self.range();
        self.samples.is_empty() && end > start
    }
}

/// A recognized token at its timeline position. Empty for models without token timestamps.
#[derive(Clone, Debug, PartialEq)]
pub struct TimedToken {
    pub text: String,
    /// Timeline index where the token starts.
    pub at: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// Text of the utterance `utterance` so far; ignore it if that utterance has moved on.
    Preview {
        track: usize,
        utterance: u64,
        span_start: usize,
        text: String,
        tokens: Vec<TimedToken>,
    },
    Final {
        track: usize,
        cut: Cut,
        text: String,
        /// Tokens inside the cut's span.
        tokens: Vec<TimedToken>,
    },
}
/// Recent audio of one track: `samples[0]` is timeline index `base`.
#[derive(Default)]
struct Timeline {
    base: usize,
    samples: Vec<f32>,
}

impl Timeline {
    fn end(&self) -> usize {
        self.base + self.samples.len()
    }

    fn slice(&self, start: usize, end: usize) -> &[f32] {
        &self.samples[start - self.base..end - self.base]
    }
}

struct TrackState {
    detector: Box<dyn SpeechDetector>,
    segmenter: Segmenter,
    timeline: Timeline,
    fed: usize,
    /// Changes whenever the current utterance ends or is cut, so stale previews are dropped.
    utterance: u64,
    finals_in_flight: usize,
    /// No previews for this utterance (its preview repeated the other track: speaker bleed).
    muted: Option<u64>,
}

/// Drop timeline audio once this much is no longer needed (bounds memory, amortizes drains).
const TRIM_SLACK: usize = crate::SAMPLE_RATE as usize * 30;

pub struct Pipeline {
    config: PipelineConfig,
    tracks: Vec<TrackState>,
    finals: VecDeque<Job>,
    last_preview_s: f64,
    preview_interval_s: f64,
    next_preview_track: usize,
}

impl Pipeline {
    pub fn new(config: PipelineConfig, detectors: Vec<Box<dyn SpeechDetector>>) -> Self {
        let tracks = detectors
            .into_iter()
            .map(|detector| TrackState {
                detector,
                segmenter: Segmenter::new(config.segmenter.clone()),
                timeline: Timeline::default(),
                fed: 0,
                utterance: 0,
                finals_in_flight: 0,
                muted: None,
            })
            .collect();
        Self {
            preview_interval_s: config.preview_interval_s,
            config,
            tracks,
            finals: VecDeque::new(),
            last_preview_s: f64::NEG_INFINITY,
            next_preview_track: 0,
        }
    }

    /// Append 16 kHz mono samples to a track, run its detector and queue any finals.
    /// Returns the number of finals queued.
    pub fn push(&mut self, track: usize, samples: &[f32], now_s: f64) -> usize {
        let state = &mut self.tracks[track];
        state.timeline.samples.extend_from_slice(samples);
        let window = state.detector.window();
        let mut queued = 0;
        while state.fed + window <= state.timeline.end() {
            let end = state.fed + window;
            let speech = state.detector.accept(state.timeline.slice(state.fed, end));
            state.fed = end;
            let (onset, release) = (state.detector.onset_lag(), state.detector.release_lag());
            let had_region = state.segmenter.region().is_some();
            let timeline = &state.timeline;
            let cut = state
                .segmenter
                .on_window(end, speech, onset, release, |from, to| {
                    timeline.slice(from, to).to_vec()
                });
            if cut.is_some() || had_region != state.segmenter.region().is_some() {
                state.utterance += 1;
            }
            if let Some(cut) = cut {
                self.finals.push_back(final_job(track, state, cut, now_s));
                queued += 1;
            }
        }
        trim(state);
        queued
    }
    /// The next job for an idle decoder: the oldest final of the priority track, else the
    /// oldest final, else a preview if one is due.
    pub fn next_job(&mut self, now_s: f64) -> Option<Job> {
        let priority = self.config.priority_track;
        let first = self
            .finals
            .iter()
            .position(|job| Some(job.track) == priority)
            .unwrap_or(0);
        if let Some(job) = self.finals.remove(first) {
            return Some(job);
        }
        if !self.config.previews || now_s - self.last_preview_s < self.preview_interval_s {
            return None;
        }
        let eligible = |state: &TrackState| {
            state.muted != Some(state.utterance)
                && state
                    .segmenter
                    .region()
                    .is_some_and(|region| state.fed - region.span_start >= self.config.min_preview)
        };
        let count = self.tracks.len();
        let track = match self.config.priority_track {
            Some(priority) if priority < count && eligible(&self.tracks[priority]) => {
                Some(priority)
            }
            _ => (0..count)
                .map(|offset| (self.next_preview_track + offset) % count)
                .find(|&i| eligible(&self.tracks[i])),
        }?;
        self.next_preview_track = track + 1;
        self.last_preview_s = now_s;
        let state = &self.tracks[track];
        let region = state.segmenter.region()?;
        Some(Job {
            track,
            kind: JobKind::Preview {
                utterance: state.utterance,
                span_start: region.span_start,
            },
            start: region.span_start,
            samples: state.timeline.slice(region.span_start, state.fed).to_vec(),
            ready_s: now_s,
        })
    }

    /// Turn a decoded job into output. `decode_s` adapts the preview interval. Previews of an
    /// utterance that has since ended or been cut return `None`.
    pub fn complete(&mut self, job: Job, transcript: Transcript, decode_s: f64) -> Option<Output> {
        let state = &mut self.tracks[job.track];
        match job.kind {
            JobKind::Final(cut) => {
                state.finals_in_flight -= 1;
                let (text, tokens) = stitch(&cut, job.start, &transcript);
                Some(Output::Final {
                    track: job.track,
                    cut,
                    text,
                    tokens,
                })
            }
            JobKind::Preview {
                utterance,
                span_start,
            } => {
                self.preview_interval_s = self.config.preview_interval_s.max(2.0 * decode_s);
                (utterance == state.utterance).then(|| Output::Preview {
                    track: job.track,
                    utterance,
                    span_start,
                    tokens: timed(job.start, &transcript),
                    text: transcript.text,
                })
            }
        }
    }

    /// End of input: feed the partial last window and close open utterances.
    pub fn finish(&mut self, now_s: f64) -> usize {
        let mut queued = 0;
        for track in 0..self.tracks.len() {
            let window = self.tracks[track].detector.window();
            let pad = (window - self.tracks[track].timeline.end() % window) % window;
            queued += self.push(track, &vec![0.0; pad], now_s);
            let state = &mut self.tracks[track];
            if let Some(cut) = state.segmenter.finish(state.fed) {
                state.utterance += 1;
                self.finals.push_back(final_job(track, state, cut, now_s));
                queued += 1;
            }
        }
        queued
    }

    /// Finals queued or being decoded.
    pub fn pending_finals(&self) -> usize {
        self.tracks.iter().map(|state| state.finals_in_flight).sum()
    }

    pub fn pending_finals_of(&self, track: usize) -> usize {
        self.tracks[track].finals_in_flight
    }

    pub fn region(&self, track: usize) -> Option<Region> {
        self.tracks[track].segmenter.region()
    }

    pub fn utterance(&self, track: usize) -> u64 {
        self.tracks[track].utterance
    }

    /// Samples fed to the detector so far on a track.
    pub fn fed(&self, track: usize) -> usize {
        self.tracks[track].fed
    }

    /// Samples `[start, end)` of a track if they are still held.
    pub fn audio(&self, track: usize, start: usize, end: usize) -> Option<&[f32]> {
        let timeline = &self.tracks[track].timeline;
        (start >= timeline.base && start <= end && end <= timeline.end())
            .then(|| timeline.slice(start, end))
    }

    /// Samples currently held for a track (memory bound check).
    pub fn held(&self, track: usize) -> usize {
        self.tracks[track].timeline.samples.len()
    }

    /// Stop previewing the current utterance of `track` until it ends.
    pub fn mute_previews(&mut self, track: usize) {
        let state = &mut self.tracks[track];
        state.muted = Some(state.utterance);
    }

    /// Samples held by queued finals.
    pub fn queued_final_samples(&self) -> usize {
        self.finals.iter().map(|job| job.samples.len()).sum()
    }

    /// Bound the memory of a decoder backlog: free the audio of the oldest queued finals until
    /// at most `keep` samples are held, but only where `readable(track)` says the recording
    /// already holds the job's audio. Returns the samples freed. Spilled jobs keep their place
    /// and range; the caller reads the audio back before decoding.
    pub fn spill_finals(&mut self, keep: usize, readable: impl Fn(usize) -> usize) -> usize {
        let mut held = self.queued_final_samples();
        let mut freed = 0;
        for job in &mut self.finals {
            if held <= keep {
                break;
            }
            let (_, end) = job.range();
            if job.samples.is_empty() || end > readable(job.track) {
                continue;
            }
            held -= job.samples.len();
            freed += job.samples.len();
            job.samples = Vec::new();
        }
        freed
    }
}

fn final_job(track: usize, state: &mut TrackState, cut: Cut, now_s: f64) -> Job {
    state.finals_in_flight += 1;
    Job {
        track,
        kind: JobKind::Final(cut),
        start: cut.audio_start,
        samples: state
            .timeline
            .slice(cut.audio_start, cut.audio_end)
            .to_vec(),
        ready_s: now_s,
    }
}

/// Keep what the open utterance and the next onset can still reach.
fn trim(state: &mut TrackState) {
    let onset = state
        .fed
        .saturating_sub(state.detector.onset_lag())
        .max(state.segmenter.committed());
    let keep = state
        .segmenter
        .region()
        .map_or(onset, |region| region.audio_start.min(onset));
    if keep > state.timeline.base + TRIM_SLACK {
        state.timeline.samples.drain(..keep - state.timeline.base);
        state.timeline.base = keep;
    }
}

/// Tokens on the timeline; `start` is the timeline index of the decoded audio.
fn timed(start: usize, transcript: &Transcript) -> Vec<TimedToken> {
    let rate = crate::SAMPLE_RATE as f32;
    transcript
        .tokens
        .iter()
        .map(|token| TimedToken {
            text: token.text.clone(),
            at: start + (token.start_s.max(0.0) * rate) as usize,
        })
        .collect()
}

/// Text and tokens of the tokens that start inside the cut's span. Without token timestamps
/// the whole text is kept; such models must run with zero overlap.
fn stitch(cut: &Cut, start: usize, transcript: &Transcript) -> (String, Vec<TimedToken>) {
    let tokens = timed(start, transcript);
    let trims_audio = cut.audio_start < cut.span_start || cut.audio_end > cut.span_end;
    if tokens.is_empty() || !trims_audio {
        return (transcript.text.clone(), tokens);
    }
    let kept: Vec<TimedToken> = tokens
        .into_iter()
        .filter(|token| token.at >= cut.span_start && token.at < cut.span_end)
        .collect();
    (join_tokens(&kept), kept)
}

/// Text of sherpa tokens; `\u{2581}` marks a word start.
pub fn join_tokens(tokens: &[TimedToken]) -> String {
    let text: String = tokens
        .iter()
        .map(|token| token.text.replace('\u{2581}', " "))
        .collect();
    text.trim().to_owned()
}
