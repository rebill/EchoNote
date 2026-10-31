//! Pipeline behaviour on synthetic audio: tone bursts are "words", and a fake decoder emits
//! one timed token per burst onset inside the decoded audio.

use super::*;
use crate::asr::Token;

const SR: usize = 16_000;
const WINDOW: usize = 320;

/// Burst onsets (sample index) and the audio: 0.3 s tones separated by `gap_s` of silence.
fn bursts(count: usize, gap_s: f64) -> (Vec<usize>, Vec<f32>) {
    let tone = SR * 3 / 10;
    let gap = (gap_s * SR as f64) as usize;
    let mut audio = vec![0.0; SR / 2];
    let mut onsets = Vec::new();
    for _ in 0..count {
        onsets.push(audio.len());
        audio.extend((0..tone).map(|i| 0.3 * (i as f32 * 0.2).sin()));
        audio.extend(std::iter::repeat_n(0.0, gap));
    }
    audio.extend(std::iter::repeat_n(0.0, SR));
    (onsets, audio)
}

fn detector() -> Box<dyn SpeechDetector> {
    // 2 loud windows to start, 25 quiet windows (0.5 s) to end.
    Box::new(EnergyDetector::new(WINDOW, 0.05, 2, 25))
}

fn pipeline(tracks: usize, hard_cap_s: f64, overlap_s: f64) -> Pipeline {
    let config = PipelineConfig {
        previews: false,
        ..PipelineConfig::meeting(hard_cap_s, overlap_s)
    };
    Pipeline::new(config, (0..tracks).map(|_| detector()).collect())
}

/// One token per burst heard in the job's audio; a burst already under way counts at 0 s.
fn fake_decode(job: &Job, onsets: &[usize]) -> Transcript {
    let end = job.start + job.samples.len();
    let tone = SR * 3 / 10;
    let tokens: Vec<Token> = onsets
        .iter()
        .enumerate()
        .filter(|(_, onset)| **onset < end && **onset + tone > job.start)
        .map(|(index, onset)| Token {
            text: format!("\u{2581}w{index}"),
            start_s: onset.saturating_sub(job.start) as f32 / SR as f32,
        })
        .collect();
    let text = tokens
        .iter()
        .map(|token| token.text.replace('\u{2581}', " "))
        .collect::<String>()
        .trim()
        .to_owned();
    Transcript { text, tokens }
}

/// Feed `audio` in 30 ms chunks, decode every job at once, return finals in order.
fn run(
    pipeline: &mut Pipeline,
    track: usize,
    audio: &[f32],
    onsets: &[usize],
) -> Vec<(Cut, String)> {
    let mut finals = Vec::new();
    let mut drain = |pipeline: &mut Pipeline, now_s: f64| {
        while let Some(job) = pipeline.next_job(now_s) {
            let transcript = fake_decode(&job, onsets);
            if let Some(Output::Final {
                cut, text, tokens, ..
            }) = pipeline.complete(job, transcript, 0.01)
            {
                // Tokens are placed on the timeline and kept only inside the span.
                assert!(
                    tokens
                        .iter()
                        .all(|token| (cut.span_start..cut.span_end).contains(&token.at))
                );
                assert_eq!(super::join_tokens(&tokens), text);
                finals.push((cut, text));
            }
        }
    };
    for (index, chunk) in audio.chunks(480).enumerate() {
        pipeline.push(track, chunk, index as f64 * 0.03);
        drain(pipeline, index as f64 * 0.03);
    }
    pipeline.finish(1e9);
    drain(pipeline, 1e9);
    finals
}

fn words(finals: &[(Cut, String)]) -> Vec<String> {
    finals
        .iter()
        .flat_map(|(_, text)| {
            text.split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn assert_tiled(finals: &[(Cut, String)]) {
    for pair in finals.windows(2) {
        assert!(
            pair[0].0.span_end <= pair[1].0.span_start,
            "spans overlap: {:?} {:?}",
            pair[0].0,
            pair[1].0
        );
    }
    for (cut, _) in finals {
        assert!(cut.span_start < cut.span_end);
        assert!(cut.audio_start <= cut.span_start && cut.span_end <= cut.audio_end);
    }
}

#[test]
fn pauses_split_utterances_and_every_word_appears_once() {
    let (onsets, audio) = bursts(6, 1.0);
    let finals = run(&mut pipeline(1, 12.0, 0.0), 0, &audio, &onsets);
    assert_eq!(finals.len(), 6);
    assert!(finals.iter().all(|(cut, _)| !cut.forced));
    assert_tiled(&finals);
    assert_eq!(
        words(&finals),
        (0..6).map(|i| format!("w{i}")).collect::<Vec<_>>()
    );
}

#[test]
fn continuous_speech_is_cut_at_the_cap_inside_a_gap() {
    // 0.1 s gaps never end an utterance (release needs 0.5 s).
    let (onsets, audio) = bursts(60, 0.1);
    let finals = run(&mut pipeline(1, 4.0, 0.0), 0, &audio, &onsets);
    assert!(
        finals.len() >= 5,
        "expected forced cuts, got {}",
        finals.len()
    );
    assert!(finals.iter().filter(|(cut, _)| cut.forced).count() >= 4);
    assert_tiled(&finals);
    for (cut, _) in finals.iter().filter(|(cut, _)| cut.forced) {
        assert!(
            cut.audio_end - cut.audio_start <= 4 * SR + WINDOW,
            "decoded audio exceeds the cap: {cut:?}"
        );
        assert!(
            audio[cut.span_end].abs() < 1e-6,
            "forced cut is inside a tone at {}",
            cut.span_end
        );
    }
    assert_eq!(
        words(&finals),
        (0..60).map(|i| format!("w{i}")).collect::<Vec<_>>()
    );
}

#[test]
fn overlapping_cuts_keep_each_word_once() {
    let (onsets, audio) = bursts(60, 0.1);
    let finals = run(&mut pipeline(1, 4.0, 0.4), 0, &audio, &onsets);
    let forced: Vec<_> = finals.iter().filter(|(cut, _)| cut.forced).collect();
    assert!(!forced.is_empty());
    assert!(
        forced.iter().all(|(cut, _)| cut.audio_end > cut.span_end),
        "forced cuts should overlap"
    );
    assert_tiled(&finals);
    assert_eq!(
        words(&finals),
        (0..60).map(|i| format!("w{i}")).collect::<Vec<_>>()
    );
}

#[test]
fn finals_go_first_and_previews_follow_the_interval_and_priority() {
    let (_, audio) = bursts(1, 0.0);
    let audio: Vec<f32> = audio[..SR / 2]
        .iter()
        .chain(audio[SR / 2..SR / 2 + SR * 3 / 10].iter().cycle().take(SR))
        .chain(&vec![0.0; SR])
        .copied()
        .collect();
    let config = PipelineConfig {
        priority_track: Some(1),
        ..PipelineConfig::meeting(12.0, 0.0)
    };
    let mut pipeline = Pipeline::new(config, vec![detector(), detector()]);
    // Both tracks are mid-utterance after 0.5 s of silence plus 0.5 s of tone.
    let head = &audio[..SR];
    pipeline.push(0, head, 0.75);
    pipeline.push(1, head, 0.75);
    let first = pipeline.next_job(0.75).expect("preview due");
    assert!(matches!(first.kind, JobKind::Preview { .. }));
    assert_eq!(
        first.track, 1,
        "the priority track gets the shared preview slot"
    );
    assert!(
        pipeline.next_job(0.8).is_none(),
        "the interval is shared by all tracks"
    );
    // A slow decode stretches the interval to twice its duration.
    let utterance = pipeline.utterance(1);
    let output = pipeline.complete(
        first,
        Transcript {
            text: "w0".into(),
            tokens: Vec::new(),
        },
        0.5,
    );
    assert_eq!(
        output,
        Some(Output::Preview {
            track: 1,
            utterance,
            span_start: pipeline.region(1).unwrap().span_start,
            text: "w0".into(),
            tokens: Vec::new(),
        })
    );
    assert!(pipeline.next_job(0.75 + 0.9).is_none());
    assert!(pipeline.next_job(0.75 + 1.01).is_some());

    // Ending the utterance queues a final, which beats any preview; a preview decoded for the
    // ended utterance is dropped.
    let stale = Job {
        track: 0,
        kind: JobKind::Preview {
            utterance: pipeline.utterance(0),
            span_start: 0,
        },
        start: 0,
        samples: Vec::new(),
        ready_s: 0.0,
    };
    pipeline.push(0, &audio[SR..], 3.0);
    assert_eq!(pipeline.pending_finals_of(0), 1);
    let job = pipeline.next_job(10.0).unwrap();
    assert!(matches!(job.kind, JobKind::Final(_)));
    assert!(
        pipeline
            .complete(stale, Transcript::default(), 0.01)
            .is_none()
    );
    assert!(matches!(
        pipeline.complete(job, Transcript::default(), 0.01),
        Some(Output::Final { track: 0, .. })
    ));
    assert_eq!(pipeline.pending_finals(), 0);
}

#[test]
fn an_overloaded_decoder_skips_previews_but_keeps_every_final() {
    let (onsets, audio) = bursts(40, 0.6);
    let mut pipeline = Pipeline::new(PipelineConfig::meeting(12.0, 0.0), vec![detector()]);
    // Never decode while audio arrives: finals pile up, previews are never built.
    for chunk in audio.chunks(480) {
        pipeline.push(0, chunk, 0.0);
    }
    pipeline.finish(0.0);
    let mut finals = Vec::new();
    while let Some(job) = pipeline.next_job(0.0) {
        let JobKind::Final(cut) = job.kind else {
            panic!("preview built while finals were queued")
        };
        let transcript = fake_decode(&job, &onsets);
        pipeline.complete(job, transcript, 0.01);
        finals.push(cut);
    }
    assert_eq!(finals.len(), 40);
    assert_eq!(pipeline.pending_finals(), 0);
}

#[test]
fn memory_stays_bounded_over_a_long_session() {
    let (onsets, audio) = bursts(10, 1.0);
    let mut pipeline = pipeline(1, 12.0, 0.0);
    let mut count = 0;
    for _ in 0..30 {
        count += run(&mut pipeline, 0, &audio, &onsets).len();
    }
    assert_eq!(count, 300);
    assert!(
        pipeline.held(0) < SR * 45,
        "holds {} s",
        pipeline.held(0) / SR
    );
}

#[test]
fn finals_of_the_priority_track_are_decoded_first() {
    let (_, audio) = bursts(2, 1.0);
    let config = PipelineConfig {
        previews: false,
        priority_track: Some(1),
        ..PipelineConfig::meeting(12.0, 0.0)
    };
    let mut pipeline = Pipeline::new(config, vec![detector(), detector()]);
    // Track 0 finishes an utterance before track 1 does.
    let half = audio.len() / 2;
    pipeline.push(0, &audio, 0.0);
    pipeline.push(1, &audio[..half], 0.0);
    pipeline.push(1, &audio[half..], 0.0);
    let order: Vec<usize> = std::iter::from_fn(|| pipeline.next_job(0.0))
        .map(|job| job.track)
        .collect();
    assert_eq!(order, [1, 1, 0, 0]);
}
