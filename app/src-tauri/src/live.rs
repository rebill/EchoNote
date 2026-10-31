//! Turns engine events into UI deltas for the meeting being recorded, at most one every
//! 100 ms (10 Hz). Levels keep their peak between deltas; previews and notices are sent as
//! their current value; segments as they commit.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use echonote_core::engine::{EngineEvent, SessionState};
use echonote_core::store::TrackKind;

use crate::views::{Delta, FinishedView, NoticeView, SegmentView, Snapshot, merge_notice};

pub const FLUSH_EVERY: Duration = Duration::from_millis(100);

/// What the app shell does with the forwarder's output.
pub trait Sink {
    fn delta(&self, delta: &Delta);
    /// The recording state changed (tray menu, shortcut toggle).
    fn state_changed(&self, meeting: i64, state: SessionState);
    /// Meetings were added, finished or recovered.
    fn library_changed(&self);
}

#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub state: Option<(i64, SessionState)>,
    pub library_changed: bool,
}

/// The live meeting's state plus the delta being collected.
#[derive(Debug, Default)]
pub struct Forwarder {
    meeting: Option<i64>,
    state: Option<SessionState>,
    previews: [String; 2],
    notices: Vec<NoticeView>,
    elapsed_before: Duration,
    running_since: Option<Instant>,
    pending: Option<Delta>,
    /// Deltas of a meeting that ended before they were flushed.
    ready: Vec<Delta>,
}

fn index(track: TrackKind) -> usize {
    match track {
        TrackKind::Mic => 0,
        TrackKind::Sys => 1,
    }
}

impl Forwarder {
    pub fn apply(&mut self, event: &EngineEvent, now: Instant) -> Outcome {
        let mut outcome = Outcome::default();
        match event {
            EngineEvent::State { meeting, state } => {
                let new = self.meeting != Some(*meeting);
                if new {
                    self.begin(*meeting);
                }
                self.set_state(*state, now);
                self.pending(*meeting).state = Some(state.as_str());
                outcome.state = Some((*meeting, *state));
                // The meeting appeared in the library, or its row is final.
                outcome.library_changed = new || *state == SessionState::Finalized;
            }
            EngineEvent::Preview {
                meeting,
                track,
                text,
            } if self.is_live(*meeting) => {
                self.previews[index(*track)].clone_from(text);
                let previews = self.previews.clone();
                self.pending(*meeting).previews = Some(previews);
            }
            EngineEvent::Segment { meeting, segment } if self.is_live(*meeting) => {
                self.pending(*meeting)
                    .segments
                    .push(SegmentView::from(segment));
            }
            EngineEvent::Levels { meeting, mic, sys } if self.is_live(*meeting) => {
                let delta = self.pending(*meeting);
                let [old_mic, old_sys] = delta.levels.unwrap_or([0.0, 0.0]);
                delta.levels = Some([old_mic.max(*mic), old_sys.max(*sys)]);
            }
            EngineEvent::Notice { meeting, notice } if self.is_live(*meeting) => {
                crate::log::log!("notice {}", NoticeView::from(notice).kind());
                merge_notice(&mut self.notices, NoticeView::from(notice));
                let notices = self.notices.clone();
                self.pending(*meeting).notices = Some(notices);
            }
            EngineEvent::Finished { meeting, stats } if self.is_live(*meeting) => {
                self.pending(*meeting).finished = Some(FinishedView::from(stats));
                outcome.library_changed = true;
            }
            EngineEvent::Recovered { .. } => outcome.library_changed = true,
            _ => {}
        }
        outcome
    }

    fn is_live(&self, meeting: i64) -> bool {
        self.meeting == Some(meeting)
    }

    fn begin(&mut self, meeting: i64) {
        if let Some(delta) = self.pending.take()
            && !delta.is_empty()
        {
            let mut delta = delta;
            delta.elapsed_ms = self.elapsed(Instant::now());
            self.ready.push(delta);
        }
        let ready = std::mem::take(&mut self.ready);
        *self = Self {
            meeting: Some(meeting),
            ready,
            ..Self::default()
        };
    }

    fn set_state(&mut self, state: SessionState, now: Instant) {
        let running = state == SessionState::Recording;
        match (self.running_since, running) {
            (None, true) => self.running_since = Some(now),
            (Some(since), false) => {
                self.elapsed_before += now.saturating_duration_since(since);
                self.running_since = None;
            }
            _ => {}
        }
        if state != SessionState::Recording {
            // No utterance is in progress while paused or stopped.
            self.previews = Default::default();
        }
        self.state = Some(state);
    }

    fn pending(&mut self, meeting: i64) -> &mut Delta {
        self.pending.get_or_insert_with(|| Delta::new(meeting))
    }

    pub fn elapsed(&self, now: Instant) -> i64 {
        let running = self
            .running_since
            .map_or(Duration::ZERO, |since| now.saturating_duration_since(since));
        (self.elapsed_before + running).as_millis() as i64
    }

    /// The deltas collected since the last call.
    pub fn take(&mut self, now: Instant) -> Vec<Delta> {
        let mut out = std::mem::take(&mut self.ready);
        if let Some(mut delta) = self.pending.take() {
            delta.elapsed_ms = self.elapsed(now);
            out.push(delta);
        }
        out
    }

    /// The live meeting, unless none was recorded yet or it is finalized. `segments` is left
    /// for the caller to fill from the library.
    pub fn snapshot(&self, now: Instant) -> Option<Snapshot> {
        let meeting = self.meeting?;
        let state = self.state?;
        if state == SessionState::Finalized {
            return None;
        }
        Some(Snapshot {
            meeting,
            state: state.as_str(),
            elapsed_ms: self.elapsed(now),
            previews: self.previews.clone(),
            notices: self.notices.clone(),
            segments: Vec::new(),
        })
    }
}

/// Forward `events` to `sink` until the engine goes away.
pub fn run(events: Receiver<EngineEvent>, forwarder: Arc<Mutex<Forwarder>>, sink: &dyn Sink) {
    let mut next_flush = Instant::now() + FLUSH_EVERY;
    loop {
        let wait = next_flush.saturating_duration_since(Instant::now());
        let received = events.recv_timeout(wait);
        let disconnected = matches!(received, Err(RecvTimeoutError::Disconnected));
        if let Ok(event) = received {
            let outcome = match forwarder.lock() {
                Ok(mut forwarder) => forwarder.apply(&event, Instant::now()),
                Err(_) => return,
            };
            if let Some((meeting, state)) = outcome.state {
                sink.state_changed(meeting, state);
            }
            if outcome.library_changed {
                sink.library_changed();
            }
        }
        let now = Instant::now();
        if now >= next_flush || disconnected {
            let deltas = match forwarder.lock() {
                Ok(mut forwarder) => forwarder.take(now),
                Err(_) => return,
            };
            for delta in &deltas {
                sink.delta(delta);
            }
            next_flush = now + FLUSH_EVERY;
        }
        if disconnected {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use echonote_core::engine::{Notice, SessionStats};
    use echonote_core::store::{Segment, Source};

    use super::*;

    fn state(meeting: i64, state: SessionState) -> EngineEvent {
        EngineEvent::State { meeting, state }
    }

    fn levels(meeting: i64, mic: f32, sys: f32) -> EngineEvent {
        EngineEvent::Levels { meeting, mic, sys }
    }

    fn segment(meeting: i64, id: i64) -> EngineEvent {
        EngineEvent::Segment {
            meeting,
            segment: Segment {
                id,
                meeting_id: meeting,
                track: TrackKind::Sys,
                start_ms: 1_000,
                end_ms: 2_000,
                text: "hello".into(),
                raw_text: None,
                model: "fake".into(),
                source: Source::Asr,
                crosstalk: None,
                revision: 0,
            },
        }
    }

    #[test]
    fn a_delta_carries_peaks_previews_segments_and_merged_notices() {
        let t0 = Instant::now();
        let mut forwarder = Forwarder::default();
        let outcome = forwarder.apply(&state(7, SessionState::Recording), t0);
        assert_eq!(outcome.state, Some((7, SessionState::Recording)));
        assert!(outcome.library_changed, "a new meeting appears in the list");
        forwarder.apply(&levels(7, 0.2, 0.1), t0);
        forwarder.apply(&levels(7, 0.1, 0.4), t0);
        forwarder.apply(
            &EngineEvent::Preview {
                meeting: 7,
                track: TrackKind::Mic,
                text: "hel".into(),
            },
            t0,
        );
        forwarder.apply(&segment(7, 1), t0);
        let notice = |notice| EngineEvent::Notice { meeting: 7, notice };
        forwarder.apply(&notice(Notice::ModelLoading), t0);
        forwarder.apply(&notice(Notice::ModelLoaded { seconds: 1.0 }), t0);
        // Events of other meetings (recovery) are not part of the live view.
        forwarder.apply(&segment(3, 99), t0);

        let deltas = forwarder.take(t0 + Duration::from_millis(500));
        assert_eq!(deltas.len(), 1);
        let delta = &deltas[0];
        assert_eq!(delta.meeting, 7);
        assert_eq!(delta.state, Some("recording"));
        assert_eq!(delta.elapsed_ms, 500);
        assert_eq!(delta.levels, Some([0.2, 0.4]));
        assert_eq!(delta.previews, Some(["hel".into(), String::new()]));
        assert_eq!(delta.segments.iter().map(|s| s.id).collect::<Vec<_>>(), [1]);
        assert_eq!(
            delta.notices,
            Some(vec![NoticeView::ModelLoaded { seconds: 1.0 }])
        );
        assert!(forwarder.take(t0).is_empty(), "nothing new");
    }

    #[test]
    fn paused_time_is_not_counted_and_finalized_meetings_have_no_snapshot() {
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        let mut forwarder = Forwarder::default();
        forwarder.apply(&state(1, SessionState::Recording), at(0));
        forwarder.apply(&state(1, SessionState::Paused), at(1_000));
        assert_eq!(forwarder.elapsed(at(5_000)), 1_000);
        forwarder.apply(&state(1, SessionState::Recording), at(5_000));
        let snapshot = forwarder.snapshot(at(5_500)).unwrap();
        assert_eq!((snapshot.state, snapshot.elapsed_ms), ("recording", 1_500));
        forwarder.apply(&state(1, SessionState::Stopping), at(6_000));
        let outcome = forwarder.apply(&state(1, SessionState::Finalized), at(6_100));
        assert!(outcome.library_changed);
        let finished = EngineEvent::Finished {
            meeting: 1,
            stats: SessionStats::default(),
        };
        assert!(forwarder.apply(&finished, at(6_200)).library_changed);
        assert_eq!(forwarder.elapsed(at(9_000)), 2_000);
        assert!(forwarder.snapshot(at(9_000)).is_none());
    }

    #[test]
    fn a_new_meeting_flushes_what_the_last_one_had_pending() {
        let t0 = Instant::now();
        let mut forwarder = Forwarder::default();
        forwarder.apply(&state(1, SessionState::Recording), t0);
        forwarder.apply(&segment(1, 10), t0);
        forwarder.apply(&state(2, SessionState::Recording), t0);
        forwarder.apply(&segment(2, 20), t0);
        let deltas = forwarder.take(t0);
        let meetings: Vec<_> = deltas
            .iter()
            .map(|d| (d.meeting, d.segments.len()))
            .collect();
        assert_eq!(meetings, [(1, 1), (2, 1)]);
    }

    struct Counting(Mutex<Vec<Instant>>);

    impl Sink for Counting {
        fn delta(&self, _delta: &Delta) {
            self.0.lock().unwrap().push(Instant::now());
        }
        fn state_changed(&self, _meeting: i64, _state: SessionState) {}
        fn library_changed(&self) {}
    }

    #[test]
    fn deltas_go_out_at_most_ten_times_a_second() {
        let (tx, rx) = mpsc::channel();
        let sink = Arc::new(Counting(Mutex::new(Vec::new())));
        let thread = {
            let sink = sink.clone();
            std::thread::spawn(move || run(rx, Arc::new(Mutex::new(Forwarder::default())), &*sink))
        };
        tx.send(state(1, SessionState::Recording)).unwrap();
        let started = Instant::now();
        // Levels every 5 ms for a second: 200 events.
        while started.elapsed() < Duration::from_secs(1) {
            tx.send(levels(1, 0.1, 0.1)).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(tx);
        thread.join().unwrap();
        let sent = sink.0.lock().unwrap();
        assert!((8..=12).contains(&sent.len()), "{} deltas", sent.len());
        // The last flush, on disconnect, may come early; the others are 100 ms apart.
        let closest = sent[..sent.len() - 1]
            .windows(2)
            .map(|pair| pair[1].duration_since(pair[0]))
            .min()
            .unwrap();
        assert!(closest >= Duration::from_millis(95), "{closest:?}");
    }
}
