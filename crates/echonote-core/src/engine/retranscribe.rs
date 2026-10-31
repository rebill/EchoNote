//! Transcribe a saved meeting again with another (high-accuracy) model. Each segment is
//! decoded within its own boundaries from the recording, so revisions, Q&A citations and
//! corrections keep pointing at the same segments. Segments whose text is not the
//! recognizer's (edited, LLM-corrected) and crosstalk segments are left alone; segments the
//! model already transcribed are skipped, so a stopped pass continues where it ended.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use super::decoder::Model;
use super::worker::{Spill, Worker, WorkerEvent};
use super::{EngineConfig, EngineError, StoreHandle};
use crate::pipeline::{Cut, Job, JobKind};
use crate::store::{MeetingStatus, Retranscribed, Source};
use crate::text::{sanitize, strip_tags};

/// Jobs queued on the worker at once: enough to keep it busy, few enough to stop quickly.
const IN_FLIGHT: usize = 2;
/// Results written at most this far apart.
const FLUSH_EVERY: Duration = Duration::from_secs(2);
const FLUSH_SIZE: usize = 20;

/// Progress of a pass, in segments.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Retranscription {
    pub done: usize,
    pub total: usize,
    /// Segments whose text changed.
    pub changed: usize,
    /// Audio decoded so far, and the time spent loading the model and decoding it (reading
    /// the audio back included), for tuning.
    pub audio_ms: u64,
    pub load_ms: u64,
    pub decode_ms: u64,
}

fn samples(ms: i64) -> usize {
    (ms.max(0) as u64 * u64::from(crate::SAMPLE_RATE) / 1_000) as usize
}

pub(super) fn run(
    config: &EngineConfig,
    store: &StoreHandle,
    worker: &Worker,
    meeting: i64,
    model: &Model,
    stop: &dyn Fn() -> bool,
    on_progress: &mut dyn FnMut(Retranscription),
) -> Result<Retranscription, EngineError> {
    let reader = store.reader()?;
    if reader.meeting(meeting)?.status == MeetingStatus::Recording {
        return Err(EngineError::InUse);
    }
    let dir = config.meeting_dir(meeting);
    let files: Vec<_> = reader
        .tracks(meeting)?
        .into_iter()
        .filter_map(|track| {
            // Only a bare file name is accepted from the database.
            let name = std::path::Path::new(&track.file).file_name()?.to_owned();
            Some((track.kind, dir.join(name)))
        })
        .collect();
    let name = model.name();
    let mut todo: std::collections::VecDeque<_> = reader
        .segments(meeting)?
        .into_iter()
        .filter(|segment| {
            segment.source == Source::Asr
                && segment.crosstalk.is_none()
                && segment.model != name
                && segment.end_ms > segment.start_ms
        })
        .filter_map(|segment| {
            let (_, path) = files.iter().find(|(kind, _)| *kind == segment.track)?;
            Some((segment, path.clone()))
        })
        .collect();
    drop(reader);

    let mut progress = Retranscription {
        total: todo.len(),
        ..Retranscription::default()
    };
    on_progress(progress);
    if todo.is_empty() {
        return Ok(progress);
    }
    let (inbox, received) = mpsc::channel();
    let client = worker.client(model.clone(), true, move |event| {
        let _ = inbox.send(event);
    });
    // Answers Loaded or LoadFailed even after an earlier failure.
    client.load();
    let mut sent = std::collections::VecDeque::new();
    let mut batch = Vec::new();
    let mut flushed = Instant::now();
    let flush = |batch: &mut Vec<Retranscribed>| -> Result<usize, EngineError> {
        if batch.is_empty() {
            return Ok(0);
        }
        let items = std::mem::take(batch);
        let name = name.clone();
        Ok(store.call(move |store| store.apply_retranscription(meeting, &name, &items))?)
    };
    let result = loop {
        if stop() {
            break Err(EngineError::Cancelled);
        }
        while sent.len() < IN_FLIGHT
            && let Some((segment, path)) = todo.pop_front()
        {
            let (start, end) = (samples(segment.start_ms), samples(segment.end_ms));
            let cut = Cut {
                span_start: start,
                span_end: end,
                audio_start: start,
                audio_end: end,
                forced: false,
            };
            let job = Job {
                track: 0,
                kind: JobKind::Final(cut),
                start,
                samples: Vec::new(),
                ready_s: 0.0,
            };
            client.decode(job, Some(Spill { path, base: 0 }));
            sent.push_back(segment);
        }
        if sent.is_empty() {
            break Ok(());
        }
        match received.recv_timeout(Duration::from_millis(100)) {
            Ok(WorkerEvent::Decoded {
                transcript,
                decode_s,
                ..
            }) => {
                let segment = sent.pop_front().expect("one result per job, in order");
                progress.audio_ms += (segment.end_ms - segment.start_ms) as u64;
                progress.decode_ms += (decode_s * 1_000.0) as u64;
                let text = sanitize(&strip_tags(&transcript.text));
                // Nothing heard is not a reason to blank a line the live model heard.
                if !text.is_empty() {
                    progress.changed += usize::from(text != segment.text);
                    batch.push(Retranscribed {
                        segment: segment.id,
                        revision: segment.revision,
                        text,
                    });
                }
                progress.done += 1;
                on_progress(progress);
            }
            Ok(WorkerEvent::LoadFailed { error, .. }) => break Err(EngineError::ModelLoad(error)),
            Ok(WorkerEvent::Skipped { .. }) => {
                break Err(EngineError::ModelLoad("not loaded".into()));
            }
            Ok(WorkerEvent::Loaded { seconds, .. }) => {
                progress.load_ms = (seconds * 1_000.0) as u64
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break Err(EngineError::Stopped),
        }
        if batch.len() >= FLUSH_SIZE || (!batch.is_empty() && flushed.elapsed() >= FLUSH_EVERY) {
            if let Err(error) = flush(&mut batch) {
                break Err(error);
            }
            flushed = Instant::now();
        }
    };
    // What was decoded is kept, also when the pass stops early.
    let written = flush(&mut batch);
    drop(client);
    worker.unload(model);
    result?;
    written?;
    Ok(progress)
}
