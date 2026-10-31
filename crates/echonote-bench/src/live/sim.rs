use std::collections::VecDeque;
use std::time::Instant;

use echonote_core::SAMPLE_RATE;
use echonote_core::asr::{OfflineAsr, StreamingAsr};
use echonote_core::vad::Vad;
use serde::Serialize;
use sherpa_onnx::OnlineStream;

use super::PreviewMode;
use super::stats::Stats;
use crate::metrics::round3;

pub const SR: f64 = SAMPLE_RATE as f64;
const MIN_PREVIEW_SAMPLES: usize = SAMPLE_RATE as usize * 3 / 10;
/// A forced split cuts at the quietest 20 ms frame within this many samples before the cap.
const SPLIT_SEARCH: usize = SAMPLE_RATE as usize * 2;
const SPLIT_FRAME: usize = SAMPLE_RATE as usize / 50;
/// Leftovers of a VAD segment after a forced split shorter than this are not decoded.
const MIN_REMAINDER: usize = SAMPLE_RATE as usize / 4;

pub struct Track {
    pub name: &'static str,
    pub samples: Vec<f32>,
    vad: Vad,
    speech_start: Option<usize>,
    /// Samples before this index already belong to an emitted final.
    committed: usize,
    onset_s: f64,
    first_preview_done: bool,
    last_request_s: f64,
    pending_preview_s: Option<f64>,
    stream: Option<(OnlineStream, usize)>,
    stream_text: String,
}

impl Track {
    pub fn new(name: &'static str, samples: Vec<f32>, vad: Vad) -> Self {
        Self {
            name,
            samples,
            vad,
            speech_start: None,
            committed: 0,
            onset_s: 0.0,
            first_preview_done: false,
            last_request_s: f64::NEG_INFINITY,
            pending_preview_s: None,
            stream: None,
            stream_text: String::new(),
        }
    }

    fn end_utterance(&mut self) {
        self.speech_start = None;
        self.pending_preview_s = None;
        self.stream = None;
        self.stream_text.clear();
    }
}

struct FinalJob {
    track: usize,
    start_sample: usize,
    samples: Vec<f32>,
    ready_s: f64,
}

#[derive(Clone, Serialize)]
pub struct FinalSegment {
    pub track: &'static str,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub emit_s: f64,
}

/// Virtual-time model of one ASR worker serving every track. Finals have priority over
/// previews; at most one preview per track is pending and it decodes the newest audio.
pub struct Sim<'a> {
    asr: &'a OfflineAsr,
    streaming: Option<&'a StreamingAsr>,
    mode: PreviewMode,
    interval_s: f64,
    preroll: usize,
    min_speech_s: f64,
    /// Utterance length in samples that forces a final; 0 disables.
    hard_cap: usize,
    queue: VecDeque<FinalJob>,
    worker_free_s: f64,
    last_preview_decode_s: f64,
    pub stats: Stats,
    pub finals: Vec<FinalSegment>,
}

impl<'a> Sim<'a> {
    pub fn new(
        asr: &'a OfflineAsr,
        streaming: Option<&'a StreamingAsr>,
        mode: PreviewMode,
        interval_s: f64,
        preroll: usize,
        min_speech_s: f64,
        hard_cap_s: f64,
    ) -> Self {
        Self {
            asr,
            streaming,
            mode,
            interval_s,
            preroll,
            min_speech_s,
            hard_cap: (hard_cap_s * SR) as usize,
            queue: VecDeque::new(),
            worker_free_s: 0.0,
            last_preview_decode_s: 0.0,
            stats: Stats::default(),
            finals: Vec::new(),
        }
    }

    pub fn tick(&mut self, tracks: &mut [Track], tick_end: usize, window: usize) {
        let now_s = tick_end as f64 / SR;
        for (index, track) in tracks.iter_mut().enumerate() {
            let started = Instant::now();
            track
                .vad
                .accept_window(&track.samples[tick_end - window..tick_end]);
            self.stats.vad_s += started.elapsed().as_secs_f64();
            if track.vad.in_speech() && track.speech_start.is_none() {
                track.speech_start =
                    Some(tick_end.saturating_sub(self.preroll).max(track.committed));
                track.onset_s = now_s - self.min_speech_s;
                track.first_preview_done = false;
            }
            self.collect_segments(index, track, now_s);
            if !track.vad.in_speech() {
                track.end_utterance();
            }
            self.force_split(index, track, tick_end, now_s);
            match self.mode {
                PreviewMode::Redecode => self.request_preview(track, tick_end, now_s),
                PreviewMode::Streaming => self.stream_preview(track, tick_end, now_s),
                PreviewMode::None => {}
            }
        }
        self.run_worker(tracks, now_s, tick_end, true);
    }

    pub fn finish(&mut self, tracks: &mut [Track], total: usize) {
        let end_s = total as f64 / SR;
        for (index, track) in tracks.iter_mut().enumerate() {
            track.vad.flush();
            self.collect_segments(index, track, end_s);
            track.end_utterance();
        }
        self.run_worker(tracks, f64::INFINITY, total, false);
        self.stats.drain_s = (self.worker_free_s - end_s).max(0.0);
    }

    fn collect_segments(&mut self, index: usize, track: &mut Track, now_s: f64) {
        while let Some(mut segment) = track.vad.pop_segment() {
            let vad_start = segment.start_sample as usize;
            let end_sample = vad_start + segment.samples.len();
            // Drop the part a forced split already emitted.
            let skip = track
                .committed
                .saturating_sub(vad_start)
                .min(segment.samples.len());
            let start_sample = vad_start + skip;
            if end_sample - start_sample >= MIN_REMAINDER {
                let samples = segment.samples.split_off(skip);
                self.queue.push_back(FinalJob {
                    track: index,
                    start_sample,
                    samples,
                    ready_s: now_s,
                });
                self.stats.max_queue = self.stats.max_queue.max(self.queue.len());
            }
            track.committed = track.committed.max(end_sample);
            // A forced split keeps speaking: the next in-progress region starts where this one ended.
            let continues = track.vad.in_speech();
            track.end_utterance();
            if continues {
                track.speech_start = Some(end_sample);
                track.first_preview_done = true;
            }
        }
    }

    /// sherpa-onnx's max_speech_duration does not split continuous speech (media playback, fast
    /// talkers), so an utterance reaching the hard cap is finalized at its quietest recent frame.
    fn force_split(&mut self, index: usize, track: &mut Track, tick_end: usize, now_s: f64) {
        let Some(start) = track.speech_start else {
            return;
        };
        if self.hard_cap == 0 || tick_end - start < self.hard_cap {
            return;
        }
        let from = tick_end
            .saturating_sub(SPLIT_SEARCH)
            .max(start + MIN_REMAINDER);
        let mut cut = (f32::INFINITY, tick_end);
        let mut position = from;
        while position + SPLIT_FRAME <= tick_end {
            let energy: f32 = track.samples[position..position + SPLIT_FRAME]
                .iter()
                .map(|sample| sample * sample)
                .sum();
            if energy < cut.0 {
                cut = (energy, position + SPLIT_FRAME / 2);
            }
            position += SPLIT_FRAME;
        }
        let cut = cut.1;
        self.queue.push_back(FinalJob {
            track: index,
            start_sample: start,
            samples: track.samples[start..cut].to_vec(),
            ready_s: now_s,
        });
        self.stats.max_queue = self.stats.max_queue.max(self.queue.len());
        self.stats.forced_splits += 1;
        track.committed = cut;
        track.end_utterance();
        track.speech_start = Some(cut);
        track.first_preview_done = true;
    }

    /// Re-decode previews: throttle so the interval is at least twice the last decode time.
    fn request_preview(&mut self, track: &mut Track, tick_end: usize, now_s: f64) {
        let Some(start) = track.speech_start else {
            return;
        };
        if track.pending_preview_s.is_some() || tick_end - start < MIN_PREVIEW_SAMPLES {
            return;
        }
        let interval = self.interval_s.max(2.0 * self.last_preview_decode_s);
        if now_s - track.last_request_s >= interval {
            track.pending_preview_s = Some(now_s);
            track.last_request_s = now_s;
        }
    }

    /// Streaming previews run on their own thread; cost is the per-tick decode time.
    fn stream_preview(&mut self, track: &mut Track, tick_end: usize, now_s: f64) {
        let (Some(start), Some(streaming)) = (track.speech_start, self.streaming) else {
            return;
        };
        let (stream, fed) = track
            .stream
            .get_or_insert_with(|| (streaming.create_stream(), start));
        if *fed >= tick_end {
            return;
        }
        let started = Instant::now();
        let text = streaming.accept(stream, &track.samples[*fed..tick_end]);
        let elapsed = started.elapsed().as_secs_f64();
        *fed = tick_end;
        self.stats.streaming_s += elapsed;
        if text != track.stream_text {
            self.stats.preview_updates += 1;
            if !track.first_preview_done && !text.trim().is_empty() {
                self.stats
                    .first_preview_s
                    .push(now_s + elapsed - track.onset_s);
                track.first_preview_done = true;
            }
            track.stream_text = text;
        }
    }

    /// Run queued work until the worker is busy past `now_s`. Finals always go first.
    fn run_worker(&mut self, tracks: &mut [Track], now_s: f64, available: usize, previews: bool) {
        while self.worker_free_s <= now_s {
            if let Some(job) = self.queue.pop_front() {
                self.decode_final(tracks, job);
            } else if !previews || !self.decode_preview(tracks, available) {
                return;
            }
        }
    }

    fn decode_final(&mut self, tracks: &[Track], job: FinalJob) {
        let start_s = self.worker_free_s.max(job.ready_s);
        let started = Instant::now();
        let text = self.asr.decode(&job.samples);
        let elapsed = started.elapsed().as_secs_f64();
        self.worker_free_s = start_s + elapsed;
        let end_sample = job.start_sample + job.samples.len();
        self.stats.decoded_audio_s += job.samples.len() as f64 / SR;
        self.stats.final_decode_s += elapsed;
        self.stats.final_decodes += 1;
        self.stats
            .final_latency_s
            .push(self.worker_free_s - end_sample as f64 / SR);
        self.stats.queue_wait_s.push(start_s - job.ready_s);
        self.finals.push(FinalSegment {
            track: tracks[job.track].name,
            start_ms: (job.start_sample as f64 / SR * 1000.0) as u64,
            end_ms: (end_sample as f64 / SR * 1000.0) as u64,
            text,
            emit_s: round3(self.worker_free_s),
        });
    }

    /// Decode the oldest pending preview with the audio available when the worker starts it.
    /// Returns false when no preview is pending.
    fn decode_preview(&mut self, tracks: &mut [Track], available: usize) -> bool {
        let Some(track) = tracks
            .iter_mut()
            .filter(|track| track.pending_preview_s.is_some())
            .min_by(|a, b| {
                a.pending_preview_s
                    .partial_cmp(&b.pending_preview_s)
                    .unwrap()
            })
        else {
            return false;
        };
        let requested_s = track.pending_preview_s.take().unwrap_or_default();
        let Some(start) = track.speech_start else {
            self.stats.dropped_previews += 1;
            return true;
        };
        let start_s = self.worker_free_s.max(requested_s);
        let end = available.min((start_s * SR) as usize);
        if end <= start {
            self.stats.dropped_previews += 1;
            return true;
        }
        let started = Instant::now();
        let text = self.asr.decode(&track.samples[start..end]);
        let elapsed = started.elapsed().as_secs_f64();
        self.worker_free_s = start_s + elapsed;
        self.last_preview_decode_s = elapsed;
        self.stats.decoded_audio_s += (end - start) as f64 / SR;
        self.stats.preview_decode_s += elapsed;
        self.stats.preview_decodes += 1;
        self.stats
            .preview_lag_s
            .push(self.worker_free_s - end as f64 / SR);
        if !track.first_preview_done && !text.trim().is_empty() {
            self.stats
                .first_preview_s
                .push(self.worker_free_s - track.onset_s);
            track.first_preview_done = true;
        }
        true
    }
}
