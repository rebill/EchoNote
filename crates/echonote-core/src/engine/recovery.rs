//! Recordings interrupted by a crash, a kill or a power cut: their meeting is still in
//! `recording`. Each track file is cut back to its last whole page, the audio after the
//! track's transcribed-until marker is decoded from the file and transcribed like a live
//! session (crosstalk included), and the meeting is marked `recovered`.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use super::transcriber::Transcriber;
use super::worker::{Spill, Worker, WorkerEvent};
use super::{EngineConfig, EngineError, EngineEvent, StoreHandle, TRACK_KINDS};
use crate::SAMPLE_RATE;
use crate::pipeline::{Pipeline, PipelineConfig};
use crate::recording::{Reader, repair};
use crate::store::MeetingStatus;

/// Decode and feed the files this much at a time.
const CHUNK: usize = SAMPLE_RATE as usize;
/// Queued final audio before feeding pauses for the decoder.
const MAX_QUEUED: usize = SAMPLE_RATE as usize * 60;

/// Returns the number of segments added.
pub(super) fn recover_meeting(
    config: &EngineConfig,
    store: &StoreHandle,
    worker: &Worker,
    events: &Sender<EngineEvent>,
    meeting: i64,
) -> Result<usize, EngineError> {
    let reader_db = store.reader()?;
    let info = reader_db.meeting(meeting)?;
    let tracks = reader_db.tracks(meeting)?;
    drop(reader_db);
    let dir = config.meeting_dir(meeting);

    let mut readers: [Option<Reader>; 2] = [None, None];
    let mut base = [0u64; 2];
    let mut files: [Option<Spill>; 2] = [None, None];
    let mut durations = Vec::new();
    for track in &tracks {
        let index = TRACK_KINDS
            .iter()
            .position(|kind| *kind == track.kind)
            .expect("two track kinds");
        // Only a bare file name is accepted from the database.
        let Some(name) = std::path::Path::new(&track.file).file_name() else {
            continue;
        };
        let path = dir.join(name);
        let Ok(probe) = repair(&path) else {
            continue;
        };
        durations.push((track.kind, probe.duration_ms() as i64));
        let from = (track.transcribed_until_ms.max(0) as u64) * u64::from(SAMPLE_RATE) / 1_000;
        if from >= probe.samples {
            continue;
        }
        if let Ok(reader) = Reader::open_at(&path, from) {
            readers[index] = Some(reader);
            base[index] = from;
            files[index] = Some(Spill { path, base: from });
        }
    }

    let mut added = 0;
    if readers.iter().any(Option::is_some) {
        added = transcribe_tails(config, store, worker, events, meeting, readers, base, files)?;
    }
    let longest = durations.iter().map(|(_, ms)| *ms).max().unwrap_or(0);
    let ended = info.started_at_ms + longest;
    store.call(move |store| {
        store.finish_meeting(meeting, ended, MeetingStatus::Recovered, &durations)
    })?;
    Ok(added)
}

#[allow(clippy::too_many_arguments)]
fn transcribe_tails(
    config: &EngineConfig,
    store: &StoreHandle,
    worker: &Worker,
    events: &Sender<EngineEvent>,
    meeting: i64,
    mut readers: [Option<Reader>; 2],
    base: [u64; 2],
    files: [Option<Spill>; 2],
) -> Result<usize, EngineError> {
    let detectors = (0..2)
        .map(|_| (config.detector)().map_err(EngineError::Detector))
        .collect::<Result<Vec<_>, _>>()?;
    let pipeline = Pipeline::new(
        PipelineConfig {
            previews: false,
            ..config.pipeline.clone()
        },
        detectors,
    );
    let (inbox, received) = mpsc::channel();
    let client = worker.client(config.model.clone(), false, move |event| {
        let _ = inbox.send(event);
    });
    let mut transcriber = Transcriber::new(
        pipeline,
        client,
        store.clone(),
        events.clone(),
        meeting,
        base,
        files,
        config.mic_wait,
    );
    let handle = |transcriber: &mut Transcriber, event: WorkerEvent| transcriber.on_worker(event);
    let mut buffer = vec![0.0f32; CHUNK];
    let mut fed = [0u64; 2];
    loop {
        // Feed the track that is furthest behind on the shared clock, so system finals
        // arrive before the mic finals they may explain.
        let next = (0..2)
            .filter(|&index| readers[index].is_some())
            .min_by_key(|&index| base[index] + fed[index]);
        let Some(index) = next else {
            break;
        };
        let read = readers[index]
            .as_mut()
            .map_or(Ok(0), |reader| reader.read(&mut buffer))
            .unwrap_or(0);
        if read == 0 {
            readers[index] = None;
            continue;
        }
        fed[index] += read as u64;
        transcriber.push(index, &buffer[..read]);
        loop {
            while let Ok(event) = received.try_recv() {
                handle(&mut transcriber, event);
            }
            transcriber.release_mic(false);
            transcriber.dispatch();
            transcriber.commit(false);
            if transcriber.failed() || transcriber.pipeline.queued_final_samples() < MAX_QUEUED {
                break;
            }
            if let Ok(event) = received.recv_timeout(Duration::from_millis(50)) {
                handle(&mut transcriber, event);
            }
        }
    }
    transcriber.finish();
    while !transcriber.idle() && !transcriber.failed() {
        transcriber.release_mic(false);
        transcriber.dispatch();
        transcriber.commit(false);
        match received.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => handle(&mut transcriber, event),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    transcriber.release_mic(true);
    transcriber.commit(true);
    let stats = transcriber.stats;
    Ok(stats.finals[0] + stats.finals[1])
}
