//! Engine behaviour end to end on fake sources and the fake model: tones are "speech", and
//! the fake model names each 100 ms block after the octave of its pitch (220 Hz is `c`,
//! 440 Hz is `d`).

use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::*;
use crate::capture::{FakeSource, Pace, SourceFormat};
use crate::pipeline::EnergyDetector;
use crate::recording::{Writer, probe};
use crate::store::{Crosstalk, MeetingStatus, NewSegment, NewTrack, Store};
use crate::test_util::TempDir;

const SR: u32 = 16_000;

fn config(dir: &TempDir, model: FakeModel) -> EngineConfig {
    let detector: DetectorFactory = Arc::new(|| {
        // 20 ms windows, 2 loud to start, 25 quiet (0.5 s) to end.
        Ok(Box::new(EnergyDetector::new(320, 0.02, 2, 25)) as Box<dyn SpeechDetector>)
    });
    EngineConfig {
        idle_unload: Duration::from_millis(300),
        ..EngineConfig::new(dir.path().to_owned(), Arc::new(model), detector)
    }
}

/// `seconds` of interleaved audio with sine tones `(from_s, to_s, hz, amplitude)`.
fn tones(rate: u32, channels: usize, seconds: f64, parts: &[(f64, f64, f64, f32)]) -> Vec<f32> {
    let frames = (seconds * f64::from(rate)) as usize;
    let mut out = vec![0.0f32; frames * channels];
    for &(from, to, hz, amplitude) in parts {
        let start = (from * f64::from(rate)) as usize;
        let end = ((to * f64::from(rate)) as usize).min(frames);
        for frame in start..end {
            let t = frame as f64 / f64::from(rate);
            let value = amplitude * (2.0 * std::f64::consts::PI * hz * t).sin() as f32;
            for channel in 0..channels {
                out[frame * channels + channel] += value;
            }
        }
    }
    out
}

/// Plays `samples` once; reopening yields nothing more (a file that ended).
fn once(format: SourceFormat, samples: Vec<f32>, pace: Pace) -> OpenSource {
    let mut samples = Some(samples);
    Box::new(move || {
        let data = samples.take().unwrap_or_default();
        Ok(Box::new(FakeSource::start("fake", format, data, pace, None)) as Box<dyn AudioSource>)
    })
}

fn mono() -> SourceFormat {
    SourceFormat {
        rate: SR,
        channels: 1,
    }
}

fn options(mic: Option<OpenSource>, sys: Option<OpenSource>) -> SessionOptions {
    SessionOptions {
        title: "test".into(),
        language: None,
        mic,
        sys,
        auto_stop: None,
    }
}

/// Events until the meeting finishes.
fn until_finished(events: &Receiver<EngineEvent>, timeout: Duration) -> Vec<EngineEvent> {
    let deadline = Instant::now() + timeout;
    let mut seen = Vec::new();
    while let Ok(event) = events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        let done = matches!(event, EngineEvent::Finished { .. });
        seen.push(event);
        if done {
            return seen;
        }
    }
    panic!("no Finished event; got {seen:#?}");
}

fn notices(events: &[EngineEvent]) -> Vec<Notice> {
    events
        .iter()
        .filter_map(|event| match event {
            EngineEvent::Notice { notice, .. } => Some(notice.clone()),
            _ => None,
        })
        .collect()
}

fn segments(store: &Store, meeting: i64) -> Vec<Segment> {
    store.segments(meeting).unwrap()
}

#[test]
fn records_two_tracks_and_commits_finals() {
    let dir = TempDir::new();
    let (engine, events) = Engine::open(config(&dir, FakeModel::default())).unwrap();
    let mic = tones(
        SR,
        1,
        6.0,
        &[(1.0, 2.0, 220.0, 0.3), (4.0, 4.6, 220.0, 0.3)],
    );
    // System audio at 48 kHz stereo: resampled and downmixed on the way in.
    let stereo = SourceFormat {
        rate: 48_000,
        channels: 2,
    };
    let sys = tones(48_000, 2, 6.0, &[(2.5, 3.5, 440.0, 0.3)]);
    let meeting = engine
        .start(options(
            Some(once(mono(), mic, Pace::Fast)),
            Some(once(stereo, sys, Pace::Fast)),
        ))
        .unwrap();
    let seen = until_finished(&events, Duration::from_secs(30));
    engine.stop().unwrap();

    let reader = engine.store().reader().unwrap();
    let info = reader.meeting(meeting).unwrap();
    assert_eq!(info.status, MeetingStatus::Finalized);
    assert!((info.duration_ms.unwrap() - 6_000).abs() <= 40, "{info:?}");
    let found = segments(&reader, meeting);
    let mic: Vec<_> = found.iter().filter(|s| s.track == TrackKind::Mic).collect();
    let sys: Vec<_> = found.iter().filter(|s| s.track == TrackKind::Sys).collect();
    assert_eq!(mic.len(), 2, "{found:#?}");
    assert_eq!(sys.len(), 1, "{found:#?}");
    // Onset lag of the detector is 40 ms; the segmenter keeps a little lead-in.
    for (segment, start) in [(mic[0], 1_000), (mic[1], 4_000), (sys[0], 2_500)] {
        assert!(
            (segment.start_ms - start).abs() <= 300,
            "{segment:?} should start near {start}"
        );
    }
    assert!(
        mic.iter()
            .all(|s| s.text.contains('c') && !s.text.contains('d'))
    );
    assert!(sys[0].text.contains('d') && !sys[0].text.contains('c'));
    // Every committed segment was also announced.
    let announced = seen
        .iter()
        .filter(|event| matches!(event, EngineEvent::Segment { .. }))
        .count();
    assert_eq!(announced, 3);
    // Track files hold the whole timeline, and the markers cover the transcript.
    for track in reader.tracks(meeting).unwrap() {
        let path = engine.config().meeting_dir(meeting).join(&track.file);
        let probe = probe(&path).unwrap();
        assert!(probe.finished);
        assert!(
            (probe.duration_ms() as i64 - 6_000).abs() <= 40,
            "{probe:?}"
        );
        assert!(track.transcribed_until_ms > 0);
    }
    let Some(EngineEvent::Finished { stats, .. }) = seen.last() else {
        unreachable!()
    };
    assert_eq!(stats.finals, [2, 1]);
    assert_eq!(stats.dropped_frames, 0);
}

#[test]
fn speaker_bleed_on_the_mic_is_hidden_and_own_speech_kept() {
    let dir = TempDir::new();
    let (engine, events) = Engine::open(config(&dir, FakeModel::default())).unwrap();
    let sys = tones(
        SR,
        1,
        12.0,
        &[(1.0, 2.5, 440.0, 0.5), (4.0, 5.5, 440.0, 0.5)],
    );
    // The mic hears the speakers 10 dB down and 50 ms late, and the user once on their own.
    let mic = tones(
        SR,
        1,
        12.0,
        &[
            (1.05, 2.55, 440.0, 0.16),
            (4.05, 5.55, 440.0, 0.16),
            (8.0, 9.5, 220.0, 0.3),
        ],
    );
    let meeting = engine
        .start(options(
            Some(once(mono(), mic, Pace::Fast)),
            Some(once(mono(), sys, Pace::Fast)),
        ))
        .unwrap();
    let seen = until_finished(&events, Duration::from_secs(30));

    let reader = engine.store().reader().unwrap();
    let found = segments(&reader, meeting);
    let mic: Vec<_> = found.iter().filter(|s| s.track == TrackKind::Mic).collect();
    let duplicates: Vec<_> = mic
        .iter()
        .filter(|s| s.crosstalk == Some(Crosstalk::Duplicate))
        .collect();
    let own: Vec<_> = mic.iter().filter(|s| s.crosstalk.is_none()).collect();
    assert_eq!(duplicates.len(), 2, "{mic:#?}");
    assert_eq!(own.len(), 1, "{mic:#?}");
    assert!((own[0].start_ms - 8_000).abs() <= 300);
    assert!(notices(&seen).contains(&Notice::Bleed));
    // Hidden duplicates stay out of search.
    let hits = reader.search("d d d", Some(meeting), 10).unwrap();
    assert!(
        hits.iter().all(|hit| hit.track == TrackKind::Sys),
        "{hits:#?}"
    );
}

#[test]
fn a_decoder_backlog_spills_to_the_file_and_everything_is_transcribed() {
    let dir = TempDir::new();
    let model = FakeModel {
        cost: 0.05,
        ..FakeModel::default()
    };
    let mut config = config(&dir, model);
    config.max_queued_s = 5.0;
    let (engine, events) = Engine::open(config).unwrap();
    let parts: Vec<_> = (0..20)
        .map(|i| {
            let from = 1.0 + f64::from(i) * 3.0;
            (from, from + 1.5, 440.0, 0.3)
        })
        .collect();
    let sys = tones(SR, 1, 62.0, &parts);
    let meeting = engine
        .start(options(None, Some(once(mono(), sys, Pace::Fast))))
        .unwrap();
    let seen = until_finished(&events, Duration::from_secs(60));
    let Some(EngineEvent::Finished { stats, .. }) = seen.last() else {
        unreachable!()
    };
    assert!(stats.spilled_s > 5.0, "{stats:?}");
    let reader = engine.store().reader().unwrap();
    let found = segments(&reader, meeting);
    assert_eq!(found.len(), 20, "{found:#?}");
    // Audio read back from Opus decodes the same.
    assert!(
        found
            .iter()
            .all(|s| s.text.contains('d') && !s.text.contains('c'))
    );
    let sys_track = &reader.tracks(meeting).unwrap()[0];
    assert!(sys_track.transcribed_until_ms >= 59_000);
}

#[test]
fn a_failed_device_is_reopened_and_the_timeline_continues() {
    let dir = TempDir::new();
    let (engine, events) = Engine::open(config(&dir, FakeModel::default())).unwrap();
    let mut opened = 0;
    let mic: OpenSource = Box::new(move || {
        opened += 1;
        let source = match opened {
            // An unplugged headset: 1 s of audio, then the device is gone.
            1 => FakeSource::start(
                "headset",
                mono(),
                tones(SR, 1, 3.0, &[(0.2, 0.8, 220.0, 0.3)]),
                Pace::Real,
                Some(1.0),
            ),
            // The built-in mic at 48 kHz takes over.
            2 => FakeSource::start(
                "built-in",
                SourceFormat {
                    rate: 48_000,
                    channels: 1,
                },
                tones(48_000, 1, 1.5, &[(0.3, 1.0, 220.0, 0.3)]),
                Pace::Real,
                None,
            ),
            _ => return Err(CaptureError::NoDevice("input")),
        };
        Ok(Box::new(source) as Box<dyn AudioSource>)
    });
    let started = Instant::now();
    let meeting = engine.start(options(Some(mic), None)).unwrap();
    let seen = until_finished(&events, Duration::from_secs(20));
    let wall_ms = started.elapsed().as_millis() as i64;

    let found = notices(&seen);
    assert!(found.contains(&Notice::DeviceChanged {
        track: TrackKind::Mic
    }));
    let names: Vec<_> = found
        .iter()
        .filter_map(|notice| match notice {
            Notice::SourceStarted { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["headset", "built-in"]);
    let reader = engine.store().reader().unwrap();
    let track = &reader.tracks(meeting).unwrap()[0];
    // One continuous timeline: about as long as the session ran.
    let duration = track.duration_ms.unwrap();
    assert!((duration - wall_ms).abs() <= 250, "{duration} vs {wall_ms}");
    assert!(duration >= 2_400, "{duration}");
    let found = segments(&reader, meeting);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!((found[1].start_ms - 1_300).abs() <= 250, "{found:#?}");
}

#[test]
fn pause_leaves_the_paused_time_out_and_ends_the_utterance() {
    let dir = TempDir::new();
    let (engine, events) = Engine::open(config(&dir, FakeModel::default())).unwrap();
    let mic: OpenSource = Box::new(|| {
        // A continuous tone: every reopen starts it again.
        let audio = tones(SR, 1, 30.0, &[(0.2, 30.0, 220.0, 0.3)]);
        Ok(
            Box::new(FakeSource::start("tone", mono(), audio, Pace::Real, None))
                as Box<dyn AudioSource>,
        )
    });
    let meeting = engine.start(options(Some(mic), None)).unwrap();
    std::thread::sleep(Duration::from_millis(1_000));
    engine.pause().unwrap();
    std::thread::sleep(Duration::from_millis(1_000));
    engine.resume().unwrap();
    std::thread::sleep(Duration::from_millis(1_000));
    engine.stop().unwrap();
    let seen = until_finished(&events, Duration::from_secs(10));
    let states: Vec<_> = seen
        .iter()
        .filter_map(|event| match event {
            EngineEvent::State { state, .. } => Some(*state),
            _ => None,
        })
        .collect();
    assert_eq!(
        states,
        [
            SessionState::Recording,
            SessionState::Paused,
            SessionState::Recording,
            SessionState::Stopping,
            SessionState::Finalized
        ]
    );
    let reader = engine.store().reader().unwrap();
    let duration = reader.tracks(meeting).unwrap()[0].duration_ms.unwrap();
    // 1 s + 0.6 s padding + 1 s; the paused second is not recorded.
    assert!((duration - 2_600).abs() <= 200, "{duration}");
    // The utterance before the pause ended inside the padding (pause at about 1.0 s); the
    // tone after resume is another one, starting 0.2 s after the padding.
    let found = segments(&reader, meeting);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(found[0].end_ms <= 1_500, "{found:#?}");
    assert!(found[1].start_ms >= found[0].end_ms + 300, "{found:#?}");
}

#[test]
fn a_long_silence_stops_the_recording() {
    let dir = TempDir::new();
    let (engine, events) = Engine::open(config(&dir, FakeModel::default())).unwrap();
    // One short utterance, then silence for longer than the test waits.
    let audio = tones(SR, 1, 30.0, &[(0.2, 0.6, 220.0, 0.3)]);
    let meeting = engine
        .start(SessionOptions {
            auto_stop: Some(Duration::from_millis(800)),
            ..options(Some(once(mono(), audio, Pace::Real)), None)
        })
        .unwrap();
    let seen = until_finished(&events, Duration::from_secs(10));
    assert!(notices(&seen).contains(&Notice::AutoStopped), "{seen:#?}");
    let reader = engine.store().reader().unwrap();
    assert_eq!(segments(&reader, meeting).len(), 1);
    // Speech ends at 0.6 s and the utterance 0.5 s later; then 0.8 s of silence.
    let duration = reader.tracks(meeting).unwrap()[0].duration_ms.unwrap();
    assert!((1_500..4_000).contains(&duration), "{duration}");
}

#[test]
fn the_model_loads_while_audio_waits_and_unloads_when_idle() {
    let dir = TempDir::new();
    let model = FakeModel {
        load_time: Duration::from_millis(300),
        ..FakeModel::default()
    };
    let (engine, events) = Engine::open(config(&dir, model)).unwrap();
    assert_eq!(engine.worker().loaded(), None);
    let audio = tones(SR, 1, 3.0, &[(0.2, 1.0, 220.0, 0.3)]);
    let meeting = engine
        .start(options(Some(once(mono(), audio, Pace::Fast)), None))
        .unwrap();
    let seen = until_finished(&events, Duration::from_secs(10));
    let loaded = notices(&seen).into_iter().find_map(|notice| match notice {
        Notice::ModelLoaded { seconds } => Some(seconds),
        _ => None,
    });
    assert!(loaded.is_some_and(|seconds| seconds >= 0.25), "{seen:#?}");
    let reader = engine.store().reader().unwrap();
    assert_eq!(segments(&reader, meeting).len(), 1);
    assert!(engine.worker().loaded().is_some());
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(engine.worker().loaded(), None, "unloaded after idle_unload");
}

#[test]
fn a_model_that_fails_to_load_does_not_stop_the_recording() {
    let dir = TempDir::new();
    let model = FakeModel {
        broken: true,
        ..FakeModel::default()
    };
    let (engine, events) = Engine::open(config(&dir, model)).unwrap();
    let audio = tones(SR, 1, 3.0, &[(0.2, 1.0, 220.0, 0.3)]);
    let meeting = engine
        .start(options(Some(once(mono(), audio, Pace::Fast)), None))
        .unwrap();
    let seen = until_finished(&events, Duration::from_secs(10));
    assert!(
        notices(&seen)
            .iter()
            .any(|notice| matches!(notice, Notice::ModelFailed { .. }))
    );
    let reader = engine.store().reader().unwrap();
    let track = &reader.tracks(meeting).unwrap()[0];
    assert!((track.duration_ms.unwrap() - 3_000).abs() <= 40);
    assert_eq!(
        track.transcribed_until_ms, 0,
        "nothing claimed as transcribed"
    );
    assert!(segments(&reader, meeting).is_empty());
}

#[test]
fn no_input_and_a_missing_mic_are_errors_but_missing_system_audio_is_not() {
    let dir = TempDir::new();
    let (engine, events) = Engine::open(config(&dir, FakeModel::default())).unwrap();
    assert!(matches!(
        engine.start(options(None, None)),
        Err(EngineError::NoInput)
    ));
    let broken: OpenSource = Box::new(|| Err(CaptureError::NoDevice("input")));
    assert!(matches!(
        engine.start(options(Some(broken), None)),
        Err(EngineError::Capture(_))
    ));
    let denied: OpenSource = Box::new(|| Err(CaptureError::Denied));
    let audio = tones(SR, 1, 1.0, &[]);
    let meeting = engine
        .start(options(Some(once(mono(), audio, Pace::Fast)), Some(denied)))
        .unwrap();
    let seen = until_finished(&events, Duration::from_secs(10));
    assert!(notices(&seen).contains(&Notice::PermissionDenied {
        track: TrackKind::Sys,
    }));
    let reader = engine.store().reader().unwrap();
    let tracks = reader.tracks(meeting).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].kind, TrackKind::Mic);
    // Failed starts left no meeting behind.
    assert_eq!(reader.meetings().unwrap().len(), 1);
}

#[test]
fn a_second_recording_waits_for_the_first() {
    let dir = TempDir::new();
    let (engine, events) = Engine::open(config(&dir, FakeModel::default())).unwrap();
    let audio = tones(SR, 1, 30.0, &[]);
    engine
        .start(options(Some(once(mono(), audio.clone(), Pace::Real)), None))
        .unwrap();
    assert!(matches!(
        engine.start(options(Some(once(mono(), audio, Pace::Real)), None)),
        Err(EngineError::Busy)
    ));
    engine.stop().unwrap();
    until_finished(&events, Duration::from_secs(10));
    assert!(matches!(engine.stop(), Ok(())));
    assert!(matches!(engine.pause(), Err(EngineError::NoSession)));
}

#[test]
fn recovery_transcribes_the_tail_after_the_marker() {
    let dir = TempDir::new();
    let config = config(&dir, FakeModel::default());
    let meeting;
    {
        // A recording that crashed: the meeting is still `recording`, the file has no
        // end-of-stream page and a torn write at the end, finals are stored up to 3 s.
        let mut store = Store::open(&config.library_path()).unwrap();
        meeting = store.create_meeting("crashed", 1_000_000, None).unwrap();
        let track = NewTrack {
            kind: TrackKind::Mic,
            file: "mic.opus".into(),
            sample_rate: SR,
            device_name: None,
        };
        store.add_track(meeting, &track).unwrap();
        let first = NewSegment {
            track: TrackKind::Mic,
            start_ms: 950,
            end_ms: 1_900,
            text: "c c c".into(),
            raw_text: None,
            model: "fake".into(),
            crosstalk: None,
        };
        store
            .commit_finals(meeting, &[first], &[(TrackKind::Mic, 3_000)])
            .unwrap();
        let meeting_dir = config.meeting_dir(meeting);
        std::fs::create_dir_all(&meeting_dir).unwrap();
        let path = meeting_dir.join("mic.opus");
        let mut writer = Writer::create(&path, config.writer).unwrap();
        let audio = tones(
            SR,
            1,
            10.0,
            &[
                (1.0, 1.8, 220.0, 0.3),
                (4.0, 4.8, 220.0, 0.3),
                (7.0, 7.8, 220.0, 0.3),
            ],
        );
        writer.push(&audio).unwrap();
        drop(writer);
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"OggS\0torn").unwrap();
    }
    let (engine, events) = Engine::open(config).unwrap();
    assert_eq!(engine.recover().unwrap(), vec![meeting]);
    let reader = engine.store().reader().unwrap();
    let info = reader.meeting(meeting).unwrap();
    assert_eq!(info.status, MeetingStatus::Recovered);
    // The page being filled at the crash is lost: at most a second.
    let duration = info.duration_ms.unwrap();
    assert!((9_000..=10_000).contains(&duration), "{duration}");
    let found = segments(&reader, meeting);
    let starts: Vec<_> = found.iter().map(|s| s.start_ms).collect();
    assert_eq!(found.len(), 3, "{found:#?}");
    assert!((starts[1] - 4_000).abs() <= 300 && (starts[2] - 7_000).abs() <= 300);
    let recovered = events
        .try_iter()
        .find_map(|event| match event {
            EngineEvent::Recovered { segments, .. } => Some(segments),
            _ => None,
        })
        .unwrap();
    assert_eq!(recovered, 2);
    // Nothing is left to recover.
    assert!(engine.recover().unwrap().is_empty());
}

#[test]
fn bleed_is_still_found_when_the_decoder_lags_far_behind_the_audio() {
    // The user speaks 12 times, then the other side once (heard by the mic too). Fast sources
    // feed far ahead of the slow decoder, and the bleed final waits behind the user's finals
    // while feeding goes on: system words must not be pruned by how far audio was fed.
    let dir = TempDir::new();
    let model = FakeModel {
        cost: 0.3,
        ..FakeModel::default()
    };
    let (engine, events) = Engine::open(config(&dir, model)).unwrap();
    let mut mic_parts: Vec<_> = (0..12)
        .map(|i| {
            let from = 2.0 + f64::from(i) * 3.0;
            (from, from + 1.5, 220.0, 0.3)
        })
        .collect();
    mic_parts.push((60.05, 61.55, 440.0, 0.16));
    let mic = tones(SR, 1, 300.0, &mic_parts);
    let sys = tones(SR, 1, 300.0, &[(60.0, 61.5, 440.0, 0.5)]);
    let meeting = engine
        .start(options(
            Some(once(mono(), mic, Pace::Fast)),
            Some(once(mono(), sys, Pace::Fast)),
        ))
        .unwrap();
    until_finished(&events, Duration::from_secs(60));
    let reader = engine.store().reader().unwrap();
    let found = segments(&reader, meeting);
    let mic: Vec<_> = found.iter().filter(|s| s.track == TrackKind::Mic).collect();
    assert_eq!(mic.len(), 13, "{found:#?}");
    assert!(mic[..12].iter().all(|s| s.crosstalk.is_none()), "{mic:#?}");
    assert_eq!(mic[12].crosstalk, Some(Crosstalk::Duplicate), "{mic:#?}");
}

#[test]
fn deleting_removes_rows_and_files_but_not_a_live_or_unrecovered_meeting() {
    let dir = TempDir::new();
    let config = config(&dir, FakeModel::default());
    // Left by a crash before any audio was written.
    let crashed = Store::open(&config.library_path())
        .unwrap()
        .create_meeting("crashed", 1_000, None)
        .unwrap();
    // A delete interrupted after its rows were gone.
    let leftover = config.data_dir.join("meetings").join(".trash-99");
    std::fs::create_dir_all(&leftover).unwrap();
    let (engine, events) = Engine::open(config).unwrap();
    assert!(!leftover.exists());
    // Recovery has not run yet: the crashed meeting may still be recovered.
    assert!(matches!(
        engine.delete_meeting(crashed),
        Err(EngineError::InUse)
    ));

    let audio = tones(SR, 1, 30.0, &[(0.5, 1.5, 220.0, 0.3)]);
    let live = engine
        .start(options(Some(once(mono(), audio, Pace::Real)), None))
        .unwrap();
    std::thread::sleep(Duration::from_millis(1_500));
    assert!(matches!(
        engine.delete_meeting(live),
        Err(EngineError::InUse)
    ));
    engine.stop().unwrap();
    until_finished(&events, Duration::from_secs(10));
    let _ = engine.recover();

    let usage = engine.storage_usage();
    let bytes = engine.meeting_bytes(live);
    assert!(usage.library_bytes > 0 && bytes > 0 && usage.audio_bytes >= bytes);
    engine.delete_meeting(live).unwrap();
    assert!(!engine.config().meeting_dir(live).exists());
    let reader = engine.store().reader().unwrap();
    assert!(matches!(
        reader.meeting(live),
        Err(StoreError::NoMeeting(_))
    ));
    assert!(segments(&reader, live).is_empty());
    assert_eq!(
        engine.storage_usage().audio_bytes,
        usage.audio_bytes - bytes
    );
    engine.delete_meeting(crashed).unwrap();
    assert!(matches!(
        engine.delete_meeting(crashed),
        Err(EngineError::Store(StoreError::NoMeeting(_)))
    ));
}
