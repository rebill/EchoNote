//! The recording engine the app talks to: one recording session at a time, the ASR worker,
//! the store thread, and startup recovery of interrupted recordings.
//!
//! ```text
//! audio callbacks ─ring─▶ capture thread ──Opus──▶ meetings/<id>/{mic,sys}.opus
//!                         (timeline, pipeline) ──jobs──▶ asr-worker ──results──┐
//!                              ▲   └──finals──▶ store thread (SQLite)          │
//!                              └───────────────────────────────────────────────┘
//! ```
//!
//! Session states: `starting → recording ⇄ paused → stopping → finalized`; a meeting found
//! in `recording` at startup is repaired and transcribed from its markers, then `recovered`.
//! Events go to one channel; the app batches them for the UI.

mod decoder;
mod recovery;
mod retranscribe;
mod session;
mod store_thread;
#[cfg(test)]
mod tests;
mod transcriber;
mod worker;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use decoder::{Decoder, DecoderFactory, FAKE_TOKEN, FakeModel, Model, SherpaModel};
pub use retranscribe::Retranscription;
pub use store_thread::StoreHandle;
pub use worker::{Client, Spill, Worker, WorkerEvent};

use crate::capture::{AudioSource, CaptureError};
use crate::pipeline::{PipelineConfig, SherpaDetector, SpeechDetector};
use crate::recording::{RecordingError, WriterConfig};
use crate::store::{MeetingStatus, Segment, StoreError, TrackKind};
use crate::vad::VadSpec;

pub const MIC: usize = 0;
pub const SYS: usize = 1;
pub const TRACK_KINDS: [TrackKind; 2] = [TrackKind::Mic, TrackKind::Sys];

/// Builds one speech detector per track.
pub type DetectorFactory = Arc<dyn Fn() -> Result<Box<dyn SpeechDetector>, String> + Send + Sync>;

/// Opens (and reopens, after a device change) a track's source on the capture thread.
pub type OpenSource = Box<dyn FnMut() -> Result<Box<dyn AudioSource>, CaptureError> + Send>;

#[derive(Clone)]
pub struct EngineConfig {
    /// `library.sqlite3` and `meetings/<id>/` live here.
    pub data_dir: PathBuf,
    /// The live model.
    pub model: Model,
    pub detector: DetectorFactory,
    pub pipeline: PipelineConfig,
    /// Unload the model after this long unused (no recording holds it).
    pub idle_unload: Duration,
    /// Queued final audio kept in memory; older queued audio is read back from the file.
    pub max_queued_s: f64,
    /// How long a mic final may wait for the system track to confirm or rule out bleed.
    pub mic_wait: Duration,
    pub writer: WriterConfig,
}

impl EngineConfig {
    pub fn new(data_dir: PathBuf, model: Model, detector: DetectorFactory) -> Self {
        // Hard cap 12 s; with token timestamps cuts overlap by 0.2 s on each side.
        let overlap = if model.has_token_timestamps() {
            0.2
        } else {
            0.0
        };
        Self {
            data_dir,
            model,
            detector,
            pipeline: PipelineConfig {
                // Through speakers the mic mostly hears the other side again.
                priority_track: Some(SYS),
                ..PipelineConfig::meeting(12.0, overlap)
            },
            idle_unload: Duration::from_secs(10 * 60),
            max_queued_s: 60.0,
            mic_wait: Duration::from_secs(3),
            writer: WriterConfig::default(),
        }
    }

    /// Detectors on a sherpa-onnx VAD model.
    pub fn sherpa_vad(spec: VadSpec) -> DetectorFactory {
        Arc::new(move || {
            SherpaDetector::new(&spec)
                .map(|detector| Box::new(detector) as Box<dyn SpeechDetector>)
                .map_err(|error| error.to_string())
        })
    }

    pub fn library_path(&self) -> PathBuf {
        self.data_dir.join("library.sqlite3")
    }

    pub fn meeting_dir(&self, meeting: i64) -> PathBuf {
        self.data_dir.join("meetings").join(meeting.to_string())
    }
}

/// The default for [`SessionOptions::auto_stop`].
pub const AUTO_STOP_SILENCE: Duration = Duration::from_secs(15 * 60);

/// What to record. A track without an opener is not recorded.
pub struct SessionOptions {
    pub title: String,
    pub language: Option<String>,
    pub mic: Option<OpenSource>,
    pub sys: Option<OpenSource>,
    /// Stop recording after this long without speech on either track.
    pub auto_stop: Option<Duration>,
}

impl SessionOptions {
    /// The microphone (`mic_device`, or the system default when `None`) and, optionally,
    /// system audio.
    pub fn devices(title: String, mic: bool, mic_device: Option<String>, sys: bool) -> Self {
        let mic: Option<OpenSource> = mic.then(|| {
            Box::new(move || {
                crate::capture::MicSource::open(mic_device.as_deref())
                    .map(|source| Box::new(source) as Box<dyn AudioSource>)
            }) as OpenSource
        });
        let sys: Option<OpenSource> =
            sys.then(|| Box::new(crate::capture::open_system) as OpenSource);
        Self {
            title,
            language: None,
            mic,
            sys,
            auto_stop: Some(AUTO_STOP_SILENCE),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Starting,
    Recording,
    Paused,
    Stopping,
    Finalized,
}

impl SessionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Recording => "recording",
            Self::Paused => "paused",
            Self::Stopping => "stopping",
            Self::Finalized => "finalized",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    ModelLoading,
    ModelLoaded {
        seconds: f64,
    },
    /// Recording continues; the untranscribed audio can be transcribed later.
    ModelFailed {
        error: String,
    },
    SourceStarted {
        track: TrackKind,
        name: String,
    },
    /// The track's source could not be opened; it is retried every few seconds (or, at the
    /// start of a recording, the track is left out).
    SourceFailed {
        track: TrackKind,
        error: String,
    },
    /// Like `SourceFailed`, because the user denied access in system settings.
    PermissionDenied {
        track: TrackKind,
    },
    /// The device went away or the default device changed; a new one is opened.
    DeviceChanged {
        track: TrackKind,
    },
    /// The microphone hears the other side through the speakers: suggest headphones.
    Bleed,
    AutoStopped,
    StoreFailed {
        error: String,
    },
    WriteFailed {
        track: TrackKind,
        error: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionStats {
    pub duration_ms: i64,
    /// Finals with text, per track.
    pub finals: [usize; 2],
    /// Mic finals hidden as speaker bleed, and trimmed.
    pub hidden: usize,
    pub trimmed: usize,
    /// Source frames lost to full rings.
    pub dropped_frames: u64,
    /// Silence inserted for gaps, per track.
    pub gap_ms: [u64; 2],
    /// Queued audio read back from the files instead of memory.
    pub spilled_s: f64,
    pub decode_s: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    State {
        meeting: i64,
        state: SessionState,
    },
    /// Text of the utterance in progress; empty clears it.
    Preview {
        meeting: i64,
        track: TrackKind,
        text: String,
    },
    /// A committed final.
    Segment {
        meeting: i64,
        segment: Segment,
    },
    /// Peak RMS per track over the last 100 ms, linear.
    Levels {
        meeting: i64,
        mic: f32,
        sys: f32,
    },
    Notice {
        meeting: i64,
        notice: Notice,
    },
    Finished {
        meeting: i64,
        stats: SessionStats,
    },
    Recovered {
        meeting: i64,
        segments: usize,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Capture(#[from] CaptureError),
    #[error(transparent)]
    Recording(#[from] RecordingError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("a recording is already in progress")]
    Busy,
    #[error("recovery is already running")]
    Recovering,
    #[error("the meeting is being recorded or recovered")]
    InUse,
    #[error("no recording in progress")]
    NoSession,
    #[error("no audio input is enabled")]
    NoInput,
    #[error("voice activity detector: {0}")]
    Detector(String),
    #[error("the recording thread stopped unexpectedly")]
    Stopped,
    #[error("another meeting is being transcribed again")]
    Retranscribing,
    #[error("the model could not be loaded: {0}")]
    ModelLoad(String),
    #[error("cancelled")]
    Cancelled,
}

pub struct Engine {
    config: EngineConfig,
    store: StoreHandle,
    worker: Worker,
    events: Sender<EngineEvent>,
    active: Mutex<Option<session::Handle>>,
    recovering: AtomicBool,
    /// A meeting is being transcribed again; `interrupt` stops it for a recording.
    retranscribing: AtomicBool,
    interrupt: AtomicBool,
    /// Recovery finished at least once: a meeting still marked `recording` that is not the
    /// live one could not be recovered and may be deleted.
    recovered: AtomicBool,
}

/// Bytes on disk: the library database (with its WAL) and the recordings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StorageUsage {
    pub library_bytes: u64,
    pub audio_bytes: u64,
}

/// Prefix of a meeting directory being deleted; left over ones are removed at startup.
const TRASH_PREFIX: &str = ".trash-";

fn dir_size(path: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |meta| meta.len()),
            _ => 0,
        })
        .sum()
}

impl Engine {
    /// Open the library (migrating it if needed) and start the worker and store threads.
    pub fn open(config: EngineConfig) -> Result<(Self, Receiver<EngineEvent>), EngineError> {
        crate::capture::ring::init_clock();
        let meetings = config.data_dir.join("meetings");
        std::fs::create_dir_all(&meetings)?;
        // Deletions interrupted after the rows were gone.
        for entry in std::fs::read_dir(&meetings)?.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(TRASH_PREFIX)
            {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
        let store = StoreHandle::open(&config.library_path())?;
        let worker = Worker::spawn(config.idle_unload);
        let (events, received) = mpsc::channel();
        Ok((
            Self {
                config,
                store,
                worker,
                events,
                active: Mutex::new(None),
                recovering: AtomicBool::new(false),
                retranscribing: AtomicBool::new(false),
                interrupt: AtomicBool::new(false),
                recovered: AtomicBool::new(false),
            },
            received,
        ))
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn store(&self) -> &StoreHandle {
        &self.store
    }

    pub fn worker(&self) -> &Worker {
        &self.worker
    }

    /// Another sender for the event channel (for app-level jobs that report alongside).
    pub fn events(&self) -> Sender<EngineEvent> {
        self.events.clone()
    }

    /// Start recording; returns the new meeting's id once capture runs. Fails when the
    /// microphone cannot be opened; system audio that cannot be opened is left out.
    pub fn start(&self, options: SessionOptions) -> Result<i64, EngineError> {
        let mut active = self.active.lock().map_err(|_| EngineError::Stopped)?;
        if let Some(handle) = active.as_mut() {
            if !handle.finished() {
                return Err(EngineError::Busy);
            }
            handle.join();
        }
        // Recording comes first: a pass of the high-accuracy model stops.
        self.interrupt.store(true, Ordering::Release);
        let handle = session::spawn(
            self.config.clone(),
            self.store.clone(),
            &self.worker,
            self.events.clone(),
            options,
        )?;
        let meeting = handle.meeting;
        *active = Some(handle);
        Ok(meeting)
    }

    pub fn pause(&self) -> Result<(), EngineError> {
        self.command(session::Command::Pause)
    }

    pub fn resume(&self) -> Result<(), EngineError> {
        self.command(session::Command::Resume)
    }

    /// Stop recording and wait until the remaining audio is transcribed and the files are
    /// closed.
    pub fn stop(&self) -> Result<(), EngineError> {
        let thread = {
            let mut active = self.active.lock().map_err(|_| EngineError::Stopped)?;
            let handle = active.as_mut().ok_or(EngineError::NoSession)?;
            handle.send(session::Command::Stop);
            handle.thread.take()
        };
        if let Some(thread) = thread {
            let _ = thread.join();
        }
        Ok(())
    }

    /// The current or last session.
    pub fn state(&self) -> Option<(i64, SessionState)> {
        let active = self.active.lock().ok()?;
        active
            .as_ref()
            .map(|handle| (handle.meeting, handle.state()))
    }

    fn command(&self, command: session::Command) -> Result<(), EngineError> {
        let active = self.active.lock().map_err(|_| EngineError::Stopped)?;
        match active.as_ref() {
            Some(handle) if !handle.finished() => {
                handle.send(command);
                Ok(())
            }
            _ => Err(EngineError::NoSession),
        }
    }

    /// Transcribe `meeting` again with `model` (see `retranscribe`). Blocks until done; run
    /// it on a thread of its own. Refused while recording or recovering; starting a
    /// recording stops it with `Busy`, and `stop` with `Cancelled`. What was decoded before
    /// it stopped is kept, and the next pass continues from there.
    pub fn retranscribe(
        &self,
        meeting: i64,
        model: &Model,
        stop: &dyn Fn() -> bool,
        on_progress: &mut dyn FnMut(Retranscription),
    ) -> Result<Retranscription, EngineError> {
        {
            let active = self.active.lock().map_err(|_| EngineError::Stopped)?;
            if active.as_ref().is_some_and(|handle| !handle.finished()) {
                return Err(EngineError::Busy);
            }
            if self.recovering.load(Ordering::Acquire) {
                return Err(EngineError::Recovering);
            }
            if self.retranscribing.swap(true, Ordering::AcqRel) {
                return Err(EngineError::Retranscribing);
            }
            self.interrupt.store(false, Ordering::Release);
        }
        let interrupted = || self.interrupt.load(Ordering::Acquire);
        let result = retranscribe::run(
            &self.config,
            &self.store,
            &self.worker,
            meeting,
            model,
            &|| stop() || interrupted(),
            on_progress,
        );
        self.retranscribing.store(false, Ordering::Release);
        match result {
            Err(EngineError::Cancelled) if interrupted() => Err(EngineError::Busy),
            other => other,
        }
    }

    /// Repair and finish transcribing meetings left in `recording` by a crash. Blocks until
    /// done; run it on a background thread at startup. Returns the recovered meetings.
    pub fn recover(&self) -> Result<Vec<i64>, EngineError> {
        if self.recovering.swap(true, Ordering::AcqRel) {
            return Err(EngineError::Recovering);
        }
        let result = (|| {
            // Listing under the lock keeps a session that is starting out of the list.
            let meetings = {
                let active = self.active.lock().map_err(|_| EngineError::Stopped)?;
                let current = active
                    .as_ref()
                    .filter(|handle| !handle.finished())
                    .map(|handle| handle.meeting);
                let meetings = self.store.reader()?.unfinished_meetings()?;
                meetings
                    .into_iter()
                    .filter(|meeting| Some(meeting.id) != current)
                    .collect::<Vec<_>>()
            };
            let mut recovered = Vec::new();
            for meeting in meetings {
                let segments = recovery::recover_meeting(
                    &self.config,
                    &self.store,
                    &self.worker,
                    &self.events,
                    meeting.id,
                )?;
                let _ = self.events.send(EngineEvent::Recovered {
                    meeting: meeting.id,
                    segments,
                });
                recovered.push(meeting.id);
            }
            Ok(recovered)
        })();
        self.recovered.store(true, Ordering::Release);
        self.recovering.store(false, Ordering::Release);
        result
    }

    /// Delete a meeting's rows and recordings. Refused for the meeting being recorded, and
    /// for unfinished meetings until recovery has had its turn. The directory is renamed
    /// first, so a failure leaves either everything or nothing but a trash folder.
    pub fn delete_meeting(&self, meeting: i64) -> Result<(), EngineError> {
        let active = self.active.lock().map_err(|_| EngineError::Stopped)?;
        if active
            .as_ref()
            .is_some_and(|handle| handle.meeting == meeting && !handle.finished())
        {
            return Err(EngineError::InUse);
        }
        let status = self.store.reader()?.meeting(meeting)?.status;
        if status == MeetingStatus::Recording
            && (self.recovering.load(Ordering::Acquire) || !self.recovered.load(Ordering::Acquire))
        {
            return Err(EngineError::InUse);
        }
        let dir = self.config.meeting_dir(meeting);
        let trash = dir.with_file_name(format!("{TRASH_PREFIX}{meeting}"));
        let moved = match std::fs::rename(&dir, &trash) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if let Err(error) = self.store.call(move |store| store.delete_meeting(meeting)) {
            if moved {
                let _ = std::fs::rename(&trash, &dir);
            }
            return Err(error.into());
        }
        drop(active);
        if moved {
            std::fs::remove_dir_all(&trash)?;
        }
        Ok(())
    }

    /// Bytes used by the library and all recordings.
    pub fn storage_usage(&self) -> StorageUsage {
        let library = self.config.library_path();
        let library_bytes = ["", "-wal", "-shm"]
            .iter()
            .filter_map(|suffix| {
                let mut path = library.clone().into_os_string();
                path.push(suffix);
                std::fs::metadata(path).ok()
            })
            .map(|meta| meta.len())
            .sum();
        StorageUsage {
            library_bytes,
            audio_bytes: dir_size(&self.config.data_dir.join("meetings")),
        }
    }

    /// Bytes of one meeting's recordings.
    pub fn meeting_bytes(&self, meeting: i64) -> u64 {
        dir_size(&self.config.meeting_dir(meeting))
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
