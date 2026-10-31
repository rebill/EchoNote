//! One recording session on its own thread (the capture thread of the design): every 20 ms it
//! drains each source, places the audio on the track timeline, writes it to the track file
//! and feeds the pipeline. Commands and decoder results arrive in one inbox, so the thread
//! waits on a single channel.
//!
//! Pause drops the sources and appends 0.6 s of silence, so the utterance in progress ends;
//! the paused time is left out of the timeline. Resume reopens the sources. A source that
//! fails (device gone, default device changed) is reopened at once and then every 2 s; until
//! then the timeline is filled with silence so both tracks stay on one clock.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::transcriber::Transcriber;
use super::worker::{Client, Spill, Worker, WorkerEvent};
use super::{
    EngineConfig, EngineError, EngineEvent, MIC, Notice, OpenSource, SYS, SessionOptions,
    SessionState, SessionStats, StoreHandle, TRACK_KINDS,
};
use crate::SAMPLE_RATE;
use crate::capture::{AudioSource, KeepAwake, Stamp, Timeline, TimelineConfig, now_ns};
use crate::pipeline::Pipeline;
use crate::recording::{Syncer, Writer};
use crate::store::{MeetingStatus, NewTrack, TrackKind};

const TICK: Duration = Duration::from_millis(20);
const LEVELS_EVERY: Duration = Duration::from_millis(100);
/// A source that delivers nothing this long gets silence, so the pipeline keeps moving
/// (WASAPI loopback without packets, a device being reopened).
const IDLE_FILL_AFTER: Duration = Duration::from_millis(500);
/// Idle fill stops this far short of now, so buffers that are merely late still fit.
const IDLE_MARGIN_NS: u64 = 200_000_000;
const REOPEN_EVERY: Duration = Duration::from_secs(2);
/// Silence appended before a pause so the segmenter ends the utterance in progress.
const PAUSE_PAD_S: f64 = 0.6;
const SYNC_EVERY: Duration = Duration::from_secs(10);

pub enum Command {
    Pause,
    Resume,
    Stop,
}

pub(super) enum Inbox {
    Command(Command),
    Worker(WorkerEvent),
}

pub struct Handle {
    pub meeting: i64,
    inbox: Sender<Inbox>,
    state: Arc<Mutex<SessionState>>,
    pub thread: Option<JoinHandle<()>>,
}

impl Handle {
    pub fn send(&self, command: Command) {
        let _ = self.inbox.send(Inbox::Command(command));
    }

    pub fn state(&self) -> SessionState {
        self.state
            .lock()
            .map_or(SessionState::Finalized, |state| *state)
    }

    pub fn finished(&self) -> bool {
        self.state() == SessionState::Finalized
    }

    pub fn join(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Marks the session finalized when its thread ends, even by a panic.
struct Finalize(Arc<Mutex<SessionState>>);

impl Drop for Finalize {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.lock() {
            *state = SessionState::Finalized;
        }
    }
}

pub(super) fn spawn(
    config: EngineConfig,
    store: StoreHandle,
    worker: &Worker,
    events: Sender<EngineEvent>,
    options: SessionOptions,
) -> Result<Handle, EngineError> {
    let (inbox, received) = mpsc::channel();
    let state = Arc::new(Mutex::new(SessionState::Starting));
    let reply = inbox.clone();
    let client = worker.client(config.model.clone(), true, move |event| {
        let _ = reply.send(Inbox::Worker(event));
    });
    let (ready, started) = mpsc::channel();
    let thread = {
        let state = state.clone();
        std::thread::Builder::new()
            .name("capture".into())
            .spawn(move || {
                let _finalize = Finalize(state.clone());
                match Session::start(config, store, client, events, options, state) {
                    Ok(mut session) => {
                        let _ = ready.send(Ok(session.meeting));
                        session.run(&received);
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                    }
                }
            })?
    };
    match started.recv() {
        Ok(Ok(meeting)) => Ok(Handle {
            meeting,
            inbox,
            state,
            thread: Some(thread),
        }),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(_) => {
            let _ = thread.join();
            Err(EngineError::Stopped)
        }
    }
}

struct Track {
    open: OpenSource,
    source: Option<Box<dyn AudioSource>>,
    timeline: Timeline,
    writer: Option<Writer>,
    samples: Vec<f32>,
    stamps: Vec<Stamp>,
    /// 16 kHz output of the current step.
    out: Vec<f32>,
    last_data: Instant,
    /// Reopen the source at this time.
    retry_at: Option<Instant>,
    /// The last reopen failed (notified once until a source opens again).
    failing: bool,
    /// A finite source delivered everything.
    done: bool,
    /// Frames dropped by sources already closed.
    dropped: u64,
    level: f32,
}

impl Track {
    fn dropped(&self) -> u64 {
        self.dropped + self.source.as_ref().map_or(0, |source| source.dropped())
    }
}

struct Session {
    config: EngineConfig,
    store: StoreHandle,
    events: Sender<EngineEvent>,
    meeting: i64,
    tracks: [Option<Track>; 2],
    transcriber: Transcriber,
    syncer: Option<Syncer>,
    state: Arc<Mutex<SessionState>>,
    current: SessionState,
    paused_at: Option<u64>,
    last_levels: Instant,
    last_speech: Instant,
    _awake: KeepAwake,
}

fn notify(events: &Sender<EngineEvent>, meeting: i64, notice: Notice) {
    let _ = events.send(EngineEvent::Notice { meeting, notice });
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt()
}

impl Session {
    fn start(
        config: EngineConfig,
        store: StoreHandle,
        client: Client,
        events: Sender<EngineEvent>,
        mut options: SessionOptions,
        state: Arc<Mutex<SessionState>>,
    ) -> Result<Self, EngineError> {
        if options.mic.is_none() && options.sys.is_none() {
            return Err(EngineError::NoInput);
        }
        let detectors = (0..2)
            .map(|_| (config.detector)().map_err(EngineError::Detector))
            .collect::<Result<Vec<_>, _>>()?;
        // Timeline sample 0 is now; sources opened below deliver after this.
        let start_ns = now_ns();
        let mut opened: [Option<(OpenSource, Box<dyn AudioSource>)>; 2] = [None, None];
        if let Some(mut open) = options.mic.take() {
            // A requested microphone that cannot be opened is an error: recording without
            // it would silently lose the user's own words.
            let source = open()?;
            opened[MIC] = Some((open, source));
        }
        let mut sys_error = None;
        if let Some(mut open) = options.sys.take() {
            match open() {
                Ok(source) => opened[SYS] = Some((open, source)),
                Err(error) => sys_error = Some(error),
            }
        }
        if opened.iter().all(Option::is_none) {
            return Err(sys_error.map_or(EngineError::NoInput, EngineError::Capture));
        }

        let title = options.title.clone();
        let language = options.language.clone();
        let started_at = now_ms();
        let meeting = store
            .call(move |store| store.create_meeting(&title, started_at, language.as_deref()))?;
        let dir = config.meeting_dir(meeting);
        let names: Vec<(TrackKind, String)> = opened
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                entry
                    .as_ref()
                    .map(|(_, source)| (TRACK_KINDS[index], source.name().to_owned()))
            })
            .collect();
        let created = create_tracks(&config, &store, meeting, &dir, opened, start_ns);
        let (tracks, syncer) = match created {
            Ok(created) => created,
            Err(error) => {
                let _ = store.call(move |store| store.delete_meeting(meeting));
                let _ = std::fs::remove_dir_all(&dir);
                return Err(error);
            }
        };

        let files = [MIC, SYS].map(|index| {
            tracks[index].as_ref().map(|_| Spill {
                path: track_path(&dir, TRACK_KINDS[index]),
                base: 0,
            })
        });
        let pipeline = Pipeline::new(config.pipeline.clone(), detectors);
        let transcriber = Transcriber::new(
            pipeline,
            client,
            store.clone(),
            events.clone(),
            meeting,
            [0, 0],
            files,
            config.mic_wait,
        );
        let mut session = Self {
            config,
            store,
            events,
            meeting,
            tracks,
            transcriber,
            syncer: Some(syncer),
            state,
            current: SessionState::Starting,
            paused_at: None,
            last_levels: Instant::now(),
            last_speech: Instant::now(),
            _awake: KeepAwake::new(),
        };
        session.set_state(SessionState::Recording);
        for (track, name) in names {
            notify(
                &session.events,
                meeting,
                Notice::SourceStarted { track, name },
            );
        }
        if let Some(error) = sys_error {
            notify(
                &session.events,
                meeting,
                Notice::SourceFailed {
                    track: TrackKind::Sys,
                    error: error.to_string(),
                },
            );
        }
        session.transcriber.load();
        Ok(session)
    }

    fn set_state(&mut self, state: SessionState) {
        self.current = state;
        if let Ok(mut shared) = self.state.lock() {
            *shared = state;
        }
        let _ = self.events.send(EngineEvent::State {
            meeting: self.meeting,
            state,
        });
    }

    fn run(&mut self, inbox: &Receiver<Inbox>) {
        let mut next_tick = Instant::now();
        let mut stop = false;
        while !stop {
            let wait = next_tick.saturating_duration_since(Instant::now());
            match inbox.recv_timeout(wait) {
                Ok(message) => stop |= self.handle(message),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => stop = true,
            }
            while let Ok(message) = inbox.try_recv() {
                stop |= self.handle(message);
            }
            // A decoder result frees the worker: give it the next job right away.
            self.transcriber.dispatch();
            let now = Instant::now();
            if now >= next_tick {
                next_tick = (next_tick + TICK).max(now);
                stop |= self.tick();
            }
        }
        self.stop(inbox);
    }

    /// Returns whether the session should stop.
    fn handle(&mut self, message: Inbox) -> bool {
        match message {
            Inbox::Worker(event) => self.transcriber.on_worker(event),
            Inbox::Command(Command::Stop) => return true,
            Inbox::Command(Command::Pause) => self.pause(),
            Inbox::Command(Command::Resume) => self.resume(),
        }
        false
    }

    fn tick(&mut self) -> bool {
        if self.current == SessionState::Recording {
            for index in [MIC, SYS] {
                self.pull(index);
            }
        }
        self.transcriber.release_mic(false);
        self.transcriber.dispatch();
        self.spill();
        self.transcriber.commit(false);
        self.levels();
        if self.current != SessionState::Recording {
            return false;
        }
        let pipeline = &self.transcriber.pipeline;
        if pipeline.region(MIC).is_some() || pipeline.region(SYS).is_some() {
            self.last_speech = Instant::now();
        }
        if let Some(limit) = self.config.auto_stop_silence
            && self.last_speech.elapsed() >= limit
        {
            notify(&self.events, self.meeting, Notice::AutoStopped);
            return true;
        }
        // File and test sources end; live devices never do.
        self.tracks.iter().flatten().all(|track| track.done)
    }

    /// Drain one track's source onto its timeline and pass the result on.
    fn pull(&mut self, index: usize) {
        let meeting = self.meeting;
        let kind = TRACK_KINDS[index];
        {
            let Some(track) = self.tracks[index].as_mut() else {
                return;
            };
            if track.done {
                return;
            }
            track.out.clear();
            let now = Instant::now();
            if let Some(source) = track.source.as_mut() {
                track.samples.clear();
                track.stamps.clear();
                source.drain(&mut track.samples, &mut track.stamps);
                if !track.stamps.is_empty() {
                    track.last_data = now;
                }
                track
                    .timeline
                    .push(&track.samples, &track.stamps, &mut track.out);
                if source.failed() {
                    track.timeline.end_source(&mut track.out);
                    track.dropped = track.dropped();
                    track.source = None;
                    track.retry_at = Some(now);
                    notify(&self.events, meeting, Notice::DeviceChanged { track: kind });
                } else if source.finished() {
                    track.timeline.end_source(&mut track.out);
                    track.timeline.flush(&mut track.out);
                    track.dropped = track.dropped();
                    track.source = None;
                    track.done = true;
                }
            }
            if track.source.is_none() && !track.done && track.retry_at.is_some_and(|at| now >= at) {
                match (track.open)() {
                    Ok(source) => {
                        track.timeline.set_source(source.format(), &mut track.out);
                        let name = source.name().to_owned();
                        track.source = Some(source);
                        track.retry_at = None;
                        track.failing = false;
                        track.last_data = now;
                        notify(
                            &self.events,
                            meeting,
                            Notice::SourceStarted { track: kind, name },
                        );
                    }
                    Err(error) => {
                        track.retry_at = Some(now + REOPEN_EVERY);
                        if !track.failing {
                            track.failing = true;
                            notify(
                                &self.events,
                                meeting,
                                Notice::SourceFailed {
                                    track: kind,
                                    error: error.to_string(),
                                },
                            );
                        }
                    }
                }
            }
            if !track.done && now.duration_since(track.last_data) >= IDLE_FILL_AFTER {
                track
                    .timeline
                    .fill_to(now_ns().saturating_sub(IDLE_MARGIN_NS), &mut track.out);
            }
        }
        self.deliver(index);
    }

    /// Write a track's new 16 kHz audio to its file and feed it to the pipeline.
    fn deliver(&mut self, index: usize) {
        let Some(track) = self.tracks[index].as_mut() else {
            return;
        };
        if track.out.is_empty() {
            return;
        }
        if let Some(writer) = track.writer.as_mut()
            && let Err(error) = writer.push(&track.out)
        {
            // The file stays valid up to its last page; the transcript continues.
            track.writer = None;
            notify(
                &self.events,
                self.meeting,
                Notice::WriteFailed {
                    track: TRACK_KINDS[index],
                    error: error.to_string(),
                },
            );
        }
        track.level = track.level.max(rms(&track.out));
        self.transcriber.push(index, &track.out);
    }

    fn spill(&mut self) {
        let keep = (self.config.max_queued_s * f64::from(SAMPLE_RATE)) as usize;
        if self.transcriber.pipeline.queued_final_samples() <= keep {
            return;
        }
        let mut readable = [0usize; 2];
        for (index, track) in self.tracks.iter_mut().enumerate() {
            if let Some(writer) = track.as_mut().and_then(|track| track.writer.as_mut()) {
                // Make the newest audio readable too before choosing what to drop.
                let _ = writer.flush();
                readable[index] = writer.readable() as usize;
            }
        }
        self.transcriber.spill(self.config.max_queued_s, readable);
    }

    fn levels(&mut self) {
        if self.last_levels.elapsed() < LEVELS_EVERY {
            return;
        }
        self.last_levels = Instant::now();
        let mut take = |index: usize| {
            self.tracks[index]
                .as_mut()
                .map_or(0.0, |track| std::mem::take(&mut track.level))
        };
        let (mic, sys) = (take(MIC), take(SYS));
        let _ = self.events.send(EngineEvent::Levels {
            meeting: self.meeting,
            mic,
            sys,
        });
    }

    fn pause(&mut self) {
        if self.current != SessionState::Recording {
            return;
        }
        for index in [MIC, SYS] {
            self.pull(index);
        }
        let pad = (PAUSE_PAD_S * f64::from(SAMPLE_RATE)) as usize;
        for index in [MIC, SYS] {
            let Some(track) = self.tracks[index].as_mut() else {
                continue;
            };
            if track.done {
                continue;
            }
            track.out.clear();
            track.timeline.end_source(&mut track.out);
            track.dropped = track.dropped();
            track.source = None;
            track.retry_at = None;
            track.timeline.pad(pad, &mut track.out);
            self.deliver(index);
        }
        self.paused_at = Some(now_ns());
        self.set_state(SessionState::Paused);
    }

    fn resume(&mut self) {
        if self.current != SessionState::Paused {
            return;
        }
        let paused = now_ns().saturating_sub(self.paused_at.take().unwrap_or_else(now_ns));
        let now = Instant::now();
        for track in self.tracks.iter_mut().flatten() {
            if track.done {
                continue;
            }
            track.timeline.shift(paused);
            track.retry_at = Some(now);
            track.last_data = now;
        }
        self.last_speech = now;
        self.set_state(SessionState::Recording);
    }

    /// Capture the last audio, transcribe everything still queued, close the files.
    fn stop(&mut self, inbox: &Receiver<Inbox>) {
        let was_recording = self.current == SessionState::Recording;
        self.set_state(SessionState::Stopping);
        for index in [MIC, SYS] {
            if was_recording {
                self.pull(index);
            }
            if let Some(track) = self.tracks[index].as_mut() {
                track.out.clear();
                track.timeline.end_source(&mut track.out);
                track.timeline.flush(&mut track.out);
                track.dropped = track.dropped();
                track.source = None;
            }
            self.deliver(index);
        }
        self.transcriber.finish();
        while !self.transcriber.idle() && !self.transcriber.failed() {
            self.transcriber.release_mic(false);
            self.transcriber.dispatch();
            self.transcriber.commit(false);
            match inbox.recv_timeout(Duration::from_millis(50)) {
                Ok(Inbox::Worker(event)) => self.transcriber.on_worker(event),
                Ok(Inbox::Command(_)) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        self.transcriber.release_mic(true);
        self.transcriber.commit(true);

        let mut durations = Vec::new();
        let mut stats = SessionStats::default();
        for (index, slot) in self.tracks.iter_mut().enumerate() {
            let Some(track) = slot.as_mut() else {
                continue;
            };
            let emitted = track.timeline.emitted();
            let samples = match track.writer.take() {
                Some(writer) => writer.finish().unwrap_or(emitted),
                None => emitted,
            };
            durations.push((TRACK_KINDS[index], crate::samples_to_ms(samples) as i64));
            stats.dropped_frames += track.dropped();
            stats.gap_ms[index] = crate::samples_to_ms(track.timeline.stats().gap_samples);
        }
        if let Some(syncer) = self.syncer.take() {
            let _ = syncer.stop();
        }
        let meeting = self.meeting;
        let ended = now_ms();
        let stored = durations.clone();
        let finished = self.store.call(move |store| {
            store.finish_meeting(meeting, ended, MeetingStatus::Finalized, &stored)
        });
        if let Err(error) = finished {
            notify(
                &self.events,
                meeting,
                Notice::StoreFailed {
                    error: error.to_string(),
                },
            );
        }
        let counts = self.transcriber.stats;
        stats.duration_ms = durations.iter().map(|(_, ms)| *ms).max().unwrap_or(0);
        stats.finals = counts.finals;
        stats.hidden = counts.hidden;
        stats.trimmed = counts.trimmed;
        stats.spilled_s = counts.spilled as f64 / f64::from(SAMPLE_RATE);
        stats.decode_s = counts.decode_s;
        self.set_state(SessionState::Finalized);
        let _ = self.events.send(EngineEvent::Finished { meeting, stats });
    }
}

fn track_path(dir: &Path, kind: TrackKind) -> PathBuf {
    dir.join(format!("{}.opus", kind.as_str()))
}

type Created = ([Option<Track>; 2], Syncer);

/// Files, rows and timelines of the opened tracks.
fn create_tracks(
    config: &EngineConfig,
    store: &StoreHandle,
    meeting: i64,
    dir: &Path,
    opened: [Option<(OpenSource, Box<dyn AudioSource>)>; 2],
    start_ns: u64,
) -> Result<Created, EngineError> {
    std::fs::create_dir_all(dir)?;
    let mut tracks: [Option<Track>; 2] = [None, None];
    let mut handles = Vec::new();
    for (index, entry) in opened.into_iter().enumerate() {
        let Some((open, source)) = entry else {
            continue;
        };
        let kind = TRACK_KINDS[index];
        let writer = Writer::create(&track_path(dir, kind), config.writer)?;
        handles.push(writer.sync_handle()?);
        let new = NewTrack {
            kind,
            file: format!("{}.opus", kind.as_str()),
            sample_rate: SAMPLE_RATE,
            device_name: Some(source.name().to_owned()),
        };
        store.call(move |store| store.add_track(meeting, &new))?;
        tracks[index] = Some(Track {
            open,
            timeline: Timeline::new(source.format(), start_ns, TimelineConfig::default()),
            source: Some(source),
            writer: Some(writer),
            samples: Vec::new(),
            stamps: Vec::new(),
            out: Vec::new(),
            last_data: Instant::now(),
            retry_at: None,
            failing: false,
            done: false,
            dropped: 0,
            level: 0.0,
        });
    }
    let syncer = Syncer::spawn(dir, handles, SYNC_EVERY)?;
    Ok((tracks, syncer))
}
