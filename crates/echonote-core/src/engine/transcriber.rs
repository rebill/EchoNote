//! From pipeline output to stored segments, shared by recording sessions and recovery: hand
//! jobs to the worker, clean text, hold mic finals until the system track can rule out
//! speaker bleed, and commit batches together with each track's transcribed-until marker.

use std::collections::VecDeque;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use super::worker::{Client, Spill, WorkerEvent};
use super::{EngineEvent, MIC, Notice, SYS, TRACK_KINDS};
use crate::SAMPLE_RATE;
use crate::crosstalk::{CrosstalkConfig, Reference, Verdict, Word};
use crate::pipeline::{Output, Pipeline, TimedToken};
use crate::store::{Crosstalk, NewSegment, Segment, Source, StoreError, TrackKind};
use crate::text::{sanitize, strip_tags};

use super::StoreHandle;

/// System tokens are kept this long before the earliest mic audio still to be judged
/// (covers the acoustic lag, overlapping cuts and a capped final).
const REFERENCE_MARGIN_S: f64 = 30.0;
const COMMIT_EVERY: Duration = Duration::from_millis(500);

struct Pending {
    start: u64,
    end: u64,
    text: String,
    words: Vec<Word>,
    arrived: Instant,
}

#[derive(Default)]
struct Batch {
    segments: Vec<NewSegment>,
    /// Absolute sample each track is transcribed to, when it advanced.
    until: [Option<u64>; 2],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub finals: [usize; 2],
    pub hidden: usize,
    pub trimmed: usize,
    pub spilled: u64,
    pub decode_s: f64,
}

pub struct Transcriber {
    pub pipeline: Pipeline,
    client: Client,
    store: StoreHandle,
    events: Sender<EngineEvent>,
    meeting: i64,
    model: String,
    /// File sample of timeline index 0, per track (non-zero when recovering a tail).
    base: [u64; 2],
    /// Where spilled jobs are read back from.
    files: [Option<Spill>; 2],
    started: Instant,
    mic_wait: Duration,
    reference: Reference,
    crosstalk: CrosstalkConfig,
    pending_mic: VecDeque<Pending>,
    batch: Batch,
    last_commit: Instant,
    utterance: [u64; 2],
    preview: [String; 2],
    /// Timeline end of the last mic final released; later mic finals start after it.
    released_mic: usize,
    loaded: bool,
    failed: bool,
    busy: bool,
    bleed_noticed: bool,
    pub stats: Stats,
}

impl Transcriber {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pipeline: Pipeline,
        client: Client,
        store: StoreHandle,
        events: Sender<EngineEvent>,
        meeting: i64,
        base: [u64; 2],
        files: [Option<Spill>; 2],
        mic_wait: Duration,
    ) -> Self {
        let model = client.model().name();
        Self {
            pipeline,
            client,
            store,
            events,
            meeting,
            model,
            base,
            files,
            started: Instant::now(),
            mic_wait,
            reference: Reference::default(),
            crosstalk: CrosstalkConfig::default(),
            pending_mic: VecDeque::new(),
            batch: Batch::default(),
            last_commit: Instant::now(),
            utterance: [0; 2],
            preview: [String::new(), String::new()],
            released_mic: 0,
            loaded: false,
            failed: false,
            busy: false,
            bleed_noticed: false,
            stats: Stats::default(),
        }
    }

    pub fn now_s(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Start loading the model now (it otherwise loads with the first job).
    pub fn load(&self) {
        let _ = self.events.send(EngineEvent::Notice {
            meeting: self.meeting,
            notice: Notice::ModelLoading,
        });
        self.client.load();
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    fn seconds(&self, track: usize, sample: usize) -> f64 {
        (self.base[track] + sample as u64) as f64 / f64::from(SAMPLE_RATE)
    }

    fn ms(&self, track: usize, sample: u64) -> i64 {
        crate::samples_to_ms(self.base[track] + sample) as i64
    }

    fn words(&self, track: usize, tokens: &[TimedToken]) -> Vec<Word> {
        tokens
            .iter()
            .map(|token| Word {
                text: token.text.clone(),
                at_s: self.seconds(track, token.at),
            })
            .collect()
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.events.send(event);
    }

    fn notice(&self, notice: Notice) {
        self.emit(EngineEvent::Notice {
            meeting: self.meeting,
            notice,
        });
    }

    /// Add 16 kHz samples of a track.
    pub fn push(&mut self, track: usize, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let now_s = self.now_s();
        self.pipeline.push(track, samples, now_s);
        let utterance = self.pipeline.utterance(track);
        if utterance != self.utterance[track] {
            self.utterance[track] = utterance;
            if !self.preview[track].is_empty() {
                self.preview[track].clear();
                self.emit(EngineEvent::Preview {
                    meeting: self.meeting,
                    track: TRACK_KINDS[track],
                    text: String::new(),
                });
            }
            if track == SYS {
                self.reference.clear_preview();
            }
        }
        // Prune by the mic side, not by how far the system track was fed: with a decoder
        // backlog, system finals arrive long after their audio was fed.
        let horizon = self.seconds(MIC, self.mic_horizon());
        self.reference.prune_before(horizon - REFERENCE_MARGIN_S);
    }

    /// The earliest timeline sample a mic final still to come can start at.
    fn mic_horizon(&self) -> usize {
        if self.pipeline.pending_finals_of(MIC) > 0 || !self.pending_mic.is_empty() {
            return self.released_mic;
        }
        let fed = self.pipeline.fed(MIC);
        let next = self
            .pipeline
            .region(MIC)
            .map_or(fed, |region| region.span_start);
        next.max(self.released_mic)
    }

    pub fn on_worker(&mut self, event: WorkerEvent) {
        match event {
            WorkerEvent::Loaded { seconds, .. } => {
                self.loaded = true;
                self.notice(Notice::ModelLoaded { seconds });
            }
            WorkerEvent::LoadFailed { error, .. } => {
                self.failed = true;
                self.notice(Notice::ModelFailed { error });
            }
            WorkerEvent::Skipped { .. } => {
                self.busy = false;
                self.failed = true;
            }
            WorkerEvent::Decoded {
                job,
                transcript,
                decode_s,
            } => {
                self.busy = false;
                self.loaded = true;
                self.stats.decode_s += decode_s;
                match self.pipeline.complete(job, transcript, decode_s) {
                    Some(Output::Preview {
                        track,
                        text,
                        tokens,
                        ..
                    }) => self.on_preview(track, &text, &tokens),
                    Some(Output::Final {
                        track,
                        cut,
                        text,
                        tokens,
                    }) => {
                        let words = self.words(track, &tokens);
                        if track == SYS {
                            self.reference.add_final(&words);
                        }
                        let pending = Pending {
                            start: cut.span_start as u64,
                            end: cut.span_end as u64,
                            text: sanitize(&strip_tags(&text)),
                            words,
                            arrived: Instant::now(),
                        };
                        if track == MIC {
                            self.pending_mic.push_back(pending);
                        } else {
                            self.emit_final(track, pending, Verdict::Keep);
                        }
                    }
                    // The utterance ended or was cut while its preview decoded.
                    None => {}
                }
            }
        }
    }

    fn on_preview(&mut self, track: usize, text: &str, tokens: &[TimedToken]) {
        let words = self.words(track, tokens);
        if track == SYS {
            self.reference.set_preview(&words);
        } else if !words.is_empty()
            && self.reference.judge(&words, &self.crosstalk).verdict == Verdict::Duplicate
        {
            // The mic repeats the other side: no more previews of this utterance.
            self.pipeline.mute_previews(MIC);
            return;
        }
        let text = strip_tags(text);
        if text != self.preview[track] {
            self.preview[track] = text.clone();
            self.emit(EngineEvent::Preview {
                meeting: self.meeting,
                track: TRACK_KINDS[track],
                text,
            });
        }
    }

    fn emit_final(&mut self, track: usize, line: Pending, verdict: Verdict) {
        self.batch.until[track] = Some(self.base[track] + line.end);
        if track == MIC {
            self.released_mic = line.end as usize;
        }
        if line.text.is_empty() {
            return;
        }
        let (text, raw_text, crosstalk) = match verdict {
            Verdict::Keep => (line.text, None, None),
            Verdict::Trim { text } => (sanitize(&text), Some(line.text), Some(Crosstalk::Trim)),
            Verdict::Duplicate => (line.text, None, Some(Crosstalk::Duplicate)),
        };
        match crosstalk {
            Some(Crosstalk::Duplicate) => self.stats.hidden += 1,
            Some(Crosstalk::Trim) => self.stats.trimmed += 1,
            None => {}
        }
        if crosstalk.is_some() && !self.bleed_noticed {
            self.bleed_noticed = true;
            self.notice(Notice::Bleed);
        }
        self.stats.finals[track] += 1;
        self.batch.segments.push(NewSegment {
            track: TRACK_KINDS[track],
            start_ms: self.ms(track, line.start),
            end_ms: self.ms(track, line.end),
            text,
            raw_text,
            model: self.model.clone(),
            crosstalk,
        });
    }

    /// Release mic finals once the system track can confirm or rule out bleed, or `force`.
    pub fn release_mic(&mut self, force: bool) {
        let sys_pending = self.pipeline.pending_finals_of(SYS) > 0;
        let sys_region = self
            .pipeline
            .region(SYS)
            .map(|region| self.seconds(SYS, region.span_start));
        while let Some(line) = self.pending_mic.front() {
            let verdict = if line.words.is_empty() {
                Verdict::Keep
            } else {
                self.reference.judge(&line.words, &self.crosstalk).verdict
            };
            let end_s = (self.base[MIC] + line.end) as f64 / f64::from(SAMPLE_RATE);
            // System tokens of this moment may still arrive: a queued final, or the
            // utterance in progress if it started before this line ended.
            let may_grow = sys_pending
                || sys_region.is_some_and(|start| start <= end_s + self.crosstalk.max_lag_s);
            if verdict != Verdict::Duplicate
                && may_grow
                && !force
                && line.arrived.elapsed() < self.mic_wait
            {
                return;
            }
            let line = self.pending_mic.pop_front().expect("front exists");
            self.emit_final(MIC, line, verdict);
        }
    }

    /// Give the idle worker the next job. Nothing goes out after a load failure.
    pub fn dispatch(&mut self) {
        if self.busy || self.failed {
            return;
        }
        let now_s = self.now_s();
        if let Some(job) = self.pipeline.next_job(now_s) {
            let spill = job
                .is_spilled()
                .then(|| self.files[job.track].clone())
                .flatten();
            self.busy = true;
            self.client.decode(job, spill);
        }
    }

    /// Read queued audio back from the files instead of holding more than `max_s` of it.
    /// `readable` is how much of each track's file can be decoded now (timeline samples).
    pub fn spill(&mut self, max_s: f64, readable: [usize; 2]) {
        let keep = (max_s * f64::from(SAMPLE_RATE)) as usize;
        if self.pipeline.queued_final_samples() <= keep {
            return;
        }
        let freed = self.pipeline.spill_finals(keep, |track| readable[track]);
        self.stats.spilled += freed as u64;
    }

    /// Store finished finals and progress markers, at most every 500 ms unless `force`.
    pub fn commit(&mut self, force: bool) {
        let empty = self.batch.segments.is_empty() && self.batch.until.iter().all(Option::is_none);
        if empty || (!force && self.last_commit.elapsed() < COMMIT_EVERY) {
            return;
        }
        self.last_commit = Instant::now();
        let batch = std::mem::take(&mut self.batch);
        let meeting = self.meeting;
        let until: Vec<(TrackKind, i64)> = (0..2)
            .filter_map(|track| {
                batch.until[track]
                    .map(|sample| (TRACK_KINDS[track], crate::samples_to_ms(sample) as i64))
            })
            .collect();
        let segments = batch.segments.clone();
        let result = self
            .store
            .call(move |store| store.commit_finals(meeting, &segments, &until));
        match result {
            Ok(ids) => {
                for (id, segment) in ids.into_iter().zip(batch.segments) {
                    self.emit(EngineEvent::Segment {
                        meeting,
                        segment: Segment {
                            id,
                            meeting_id: meeting,
                            track: segment.track,
                            start_ms: segment.start_ms,
                            end_ms: segment.end_ms,
                            text: segment.text,
                            raw_text: segment.raw_text,
                            model: segment.model,
                            source: Source::Asr,
                            crosstalk: segment.crosstalk,
                            revision: 1,
                        },
                    });
                }
            }
            Err(error) => {
                // Keep the batch for the next attempt.
                self.batch.segments.splice(0..0, batch.segments);
                for track in 0..2 {
                    if self.batch.until[track].is_none() {
                        self.batch.until[track] = batch.until[track];
                    }
                }
                if !matches!(error, StoreError::Closed) {
                    self.notice(Notice::StoreFailed {
                        error: error.to_string(),
                    });
                }
            }
        }
    }

    /// End of input: close open utterances.
    pub fn finish(&mut self) {
        let now_s = self.now_s();
        self.pipeline.finish(now_s);
    }

    /// Nothing queued, decoding or waiting for release.
    pub fn idle(&self) -> bool {
        !self.busy && self.pipeline.pending_finals() == 0
    }
}
