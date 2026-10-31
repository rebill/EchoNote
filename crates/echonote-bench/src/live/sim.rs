use std::time::Instant;

use echonote_core::SAMPLE_RATE;
use echonote_core::asr::{OfflineAsr, StreamingAsr, Transcript};
use echonote_core::crosstalk::Word;
use echonote_core::pipeline::{Job, JobKind, Output, Pipeline};
use serde::Serialize;
use sherpa_onnx::OnlineStream;

use super::PreviewMode;
use super::stats::Stats;
use crate::metrics::round3;

pub const SR: f64 = SAMPLE_RATE as f64;

/// A whole input track, kept for scoring.
pub struct Track {
    pub name: &'static str,
    pub samples: Vec<f32>,
}

#[derive(Clone, Serialize)]
pub struct FinalSegment {
    pub track: &'static str,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub emit_s: f64,
    /// Tokens on the input timeline, for crosstalk matching.
    #[serde(skip)]
    pub words: Vec<Word>,
}

/// Per-track state the simulation keeps on top of the pipeline: preview latency bookkeeping
/// and the S2 streaming preview, which runs outside the pipeline on its own thread.
#[derive(Default)]
struct TrackView {
    utterance: u64,
    onset_s: f64,
    first_preview_done: bool,
    /// Streaming preview of the current utterance and the next timeline index to feed it.
    stream: Option<(OnlineStream, usize)>,
    stream_text: String,
}

/// A decode the worker has started. It goes back to the pipeline once the clock reaches `done_s`.
struct InFlight {
    job: Job,
    transcript: Transcript,
    decode_s: f64,
    done_s: f64,
}

/// Virtual-time driver of the core [`Pipeline`] with one ASR worker. Every decode runs for real
/// and its measured duration advances the worker clock; its result is handed back only when
/// the simulated clock reaches the end of that decode, so audio arriving meanwhile can make a
/// preview stale exactly as in the app.
pub struct Sim<'a> {
    pipeline: Pipeline,
    asr: &'a OfflineAsr,
    streaming: Option<&'a StreamingAsr>,
    mode: PreviewMode,
    names: Vec<&'static str>,
    min_speech_s: f64,
    views: Vec<TrackView>,
    worker_free_s: f64,
    in_flight: Option<InFlight>,
    pub stats: Stats,
    pub finals: Vec<FinalSegment>,
}

impl<'a> Sim<'a> {
    pub fn new(
        pipeline: Pipeline,
        asr: &'a OfflineAsr,
        streaming: Option<&'a StreamingAsr>,
        mode: PreviewMode,
        names: Vec<&'static str>,
        min_speech_s: f64,
    ) -> Self {
        Self {
            pipeline,
            asr,
            streaming,
            mode,
            views: names.iter().map(|_| TrackView::default()).collect(),
            names,
            min_speech_s,
            worker_free_s: 0.0,
            in_flight: None,
            stats: Stats::default(),
            finals: Vec::new(),
        }
    }

    /// Feed `[tick_end - window, tick_end)` of every track, then let the worker catch up.
    pub fn tick(&mut self, tracks: &[Track], tick_end: usize, window: usize) {
        let now_s = tick_end as f64 / SR;
        // Results due before this window arrived are delivered against the older state.
        self.complete_due(now_s);
        for (index, track) in tracks.iter().enumerate() {
            let started = Instant::now();
            self.pipeline
                .push(index, &track.samples[tick_end - window..tick_end], now_s);
            self.stats.vad_s += started.elapsed().as_secs_f64();
            self.observe(index, now_s);
        }
        self.run_worker(now_s);
    }

    pub fn finish(&mut self, total: usize) {
        let end_s = total as f64 / SR;
        self.complete_due(end_s);
        self.pipeline.finish(end_s);
        self.run_worker(f64::INFINITY);
        self.stats.drain_s = (self.worker_free_s - end_s).max(0.0);
    }

    /// Track utterance changes for preview latency, and feed the streaming preview.
    fn observe(&mut self, index: usize, now_s: f64) {
        let utterance = self.pipeline.utterance(index);
        let region = self.pipeline.region(index);
        let view = &mut self.views[index];
        if utterance != view.utterance {
            view.utterance = utterance;
            view.stream = None;
            view.stream_text.clear();
            if let Some(region) = region {
                // A region continuing after a forced cut is not a new onset.
                view.first_preview_done = region.continued;
                if !region.continued {
                    view.onset_s = now_s - self.min_speech_s;
                }
            }
        }
        let (Some(region), Some(streaming), PreviewMode::Streaming) =
            (region, self.streaming, self.mode)
        else {
            return;
        };
        let fed = self.pipeline.fed(index);
        let (stream, next) = view
            .stream
            .get_or_insert_with(|| (streaming.create_stream(), region.span_start));
        let Some(samples) = self.pipeline.audio(index, *next, fed) else {
            return;
        };
        if samples.is_empty() {
            return;
        }
        let started = Instant::now();
        let text = streaming.accept(stream, samples);
        let elapsed = started.elapsed().as_secs_f64();
        *next = fed;
        self.stats.streaming_s += elapsed;
        if text != view.stream_text {
            self.stats.preview_updates += 1;
            if !view.first_preview_done && !text.trim().is_empty() {
                self.stats
                    .first_preview_s
                    .push((self.names[index], now_s + elapsed - view.onset_s));
                view.first_preview_done = true;
            }
            view.stream_text = text;
        }
    }

    /// Start decodes until the worker is busy past `now_s` or nothing is due. Finals go first.
    fn run_worker(&mut self, now_s: f64) {
        let final_in_flight = self
            .in_flight
            .as_ref()
            .is_some_and(|in_flight| matches!(in_flight.job.kind, JobKind::Final(_)));
        let waiting = self.pipeline.pending_finals() - usize::from(final_in_flight);
        self.stats.max_queue = self.stats.max_queue.max(waiting);
        while self.complete_due(now_s) {
            let Some(job) = self.pipeline.next_job(now_s) else {
                return;
            };
            // A final that waited behind a decode starts when that decode ended.
            let start_s = self.worker_free_s.max(job.ready_s);
            let started = Instant::now();
            let transcript = self.asr.decode_transcript(&job.samples);
            let decode_s = started.elapsed().as_secs_f64();
            self.stats.decoded_audio_s += job.samples.len() as f64 / SR;
            if let JobKind::Final(_) = job.kind {
                self.stats.final_decode_s += decode_s;
                self.stats.final_decodes += 1;
                self.stats.queue_wait_s.push(start_s - job.ready_s);
            } else {
                self.stats.preview_decode_s += decode_s;
                self.stats.preview_decodes += 1;
            }
            self.worker_free_s = start_s + decode_s;
            self.in_flight = Some(InFlight {
                job,
                transcript,
                decode_s,
                done_s: self.worker_free_s,
            });
        }
    }

    /// Hand back the in-flight decode if it ended by `now_s`. Returns whether the worker is idle.
    fn complete_due(&mut self, now_s: f64) -> bool {
        if self
            .in_flight
            .as_ref()
            .is_some_and(|in_flight| in_flight.done_s > now_s)
        {
            return false;
        }
        if let Some(in_flight) = self.in_flight.take() {
            self.complete(in_flight);
        }
        true
    }

    fn complete(&mut self, in_flight: InFlight) {
        let InFlight {
            job,
            transcript,
            decode_s,
            done_s,
        } = in_flight;
        let end = job.start + job.samples.len();
        match self.pipeline.complete(job, transcript, decode_s) {
            Some(Output::Final {
                track,
                cut,
                text,
                tokens,
            }) => {
                self.stats.forced_splits += usize::from(cut.forced);
                self.stats
                    .final_latency_s
                    .push((self.names[track], done_s - cut.span_end as f64 / SR));
                self.finals.push(FinalSegment {
                    track: self.names[track],
                    start_ms: ms(cut.span_start),
                    end_ms: ms(cut.span_end),
                    text,
                    emit_s: round3(done_s),
                    words: tokens
                        .into_iter()
                        .map(|token| Word {
                            text: token.text,
                            at_s: token.at as f64 / SR,
                        })
                        .collect(),
                });
            }
            Some(Output::Preview { track, text, .. }) => {
                self.stats.preview_lag_s.push(done_s - end as f64 / SR);
                let view = &mut self.views[track];
                if !view.first_preview_done && !text.trim().is_empty() {
                    self.stats
                        .first_preview_s
                        .push((self.names[track], done_s - view.onset_s));
                    view.first_preview_done = true;
                }
            }
            // The utterance ended or was cut while this preview decoded.
            None => self.stats.dropped_previews += 1,
        }
    }
}

fn ms(sample: usize) -> u64 {
    (sample as f64 / SR * 1000.0) as u64
}
