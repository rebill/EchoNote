//! Live highlights: while recording, every [`INTERVAL`] the transcript lines finalized since
//! the last round go to the model with the current bullets, and the answer replaces them.
//! Opt-in. It runs on its own thread and only reads the library, so recording never waits
//! for it; a failed round is reported in the panel and retried next interval.

use std::time::{Duration, Instant};

use echonote_core::engine::SessionState;
use echonote_core::store::{NewSummary, SummaryKind};
use echonote_llm::highlights::{self, Update};
use echonote_llm::{Cancel, Policy};
use tauri::{AppHandle, Manager, State};

use crate::ai::config::Provider;
use crate::ai::jobs::{self, Body, Kind, Out, Ready, SummaryView};
use crate::error::CommandResult;
use crate::log::log;
use crate::state::AppState;

pub const INTERVAL: Duration = Duration::from_secs(180);
const TICK: Duration = Duration::from_secs(5);
/// Highlights should not take long; a slow model skips a round rather than piling up.
fn policy() -> Policy {
    Policy {
        attempts: 2,
        idle_timeout: Duration::from_secs(90),
        ..Policy::default()
    }
}

/// One recording. Deleting the newest meeting frees its id for the next one, so the start
/// time tells two recordings with the same id apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    meeting: i64,
    started_at_ms: i64,
}

/// The recording being followed.
struct Round {
    key: Key,
    bullets: String,
    /// Highest segment id already sent.
    last_segment: i64,
    last_run: Instant,
    /// The client is kept while the provider is unchanged, so the keychain is read once.
    client: Option<(Provider, echonote_llm::Client)>,
}

pub fn spawn(app: AppHandle) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("live-highlights".into())
        .spawn(move || run(&app))
        .map(drop)
}

fn run(app: &AppHandle) {
    let mut round: Option<Round> = None;
    loop {
        std::thread::sleep(TICK);
        let state = app.state::<AppState>();
        let Ok(engine) = state.engine() else {
            return;
        };
        let recording = engine
            .state()
            .filter(|(_, session)| *session == SessionState::Recording)
            .map(|(meeting, _)| meeting);
        let Some(meeting) = recording else {
            if engine
                .state()
                .is_none_or(|(_, session)| session == SessionState::Finalized)
            {
                round = None;
            }
            continue;
        };
        let Ok(row) = state.read(|store| store.meeting(meeting)) else {
            continue;
        };
        let key = Key {
            meeting,
            started_at_ms: row.started_at_ms,
        };
        if is_new(round.as_ref(), key) {
            round = Some(Round {
                key,
                bullets: String::new(),
                last_segment: 0,
                last_run: Instant::now(),
                client: None,
            });
        }
        let round = round.as_mut().expect("set above");
        let interval = state.dev.highlights_interval.unwrap_or(INTERVAL);
        if !state.ai().live_highlights || round.last_run.elapsed() < interval {
            continue;
        }
        round.last_run = Instant::now();
        let out = Out {
            app: app.clone(),
            job: 0,
            meeting,
            kind: Kind::Highlights,
        };
        match tauri::async_runtime::block_on(update(&state, round)) {
            Ok(Some(view)) => {
                log!("highlights updated");
                out.send(Body::Done {
                    result: serde_json::to_value(view).unwrap_or_default(),
                });
            }
            Ok(None) => {}
            Err(error) => {
                log!("highlights failed: {}", error.code);
                round.client = None;
                out.send(Body::Error { error });
            }
        }
    }
}

/// Whether `key` is another recording than the one `round` follows.
fn is_new(round: Option<&Round>, key: Key) -> bool {
    round.is_none_or(|round| round.key != key)
}

/// One round; `None` when nothing new was said.
async fn update(state: &AppState, round: &mut Round) -> CommandResult<Option<SummaryView>> {
    let provider = state.ai().active()?.clone();
    let client = match round.client.take() {
        Some((cached, client)) if cached == provider && !provider.needs_consent() => client,
        _ => {
            let Ready { client, .. } = jobs::ready(state, policy())?;
            client
        }
    };
    let (lines, terms) = jobs::transcript(state, round.key.meeting)?;
    let newest = lines.iter().map(|line| line.segment).max().unwrap_or(0);
    let new_lines: Vec<_> = lines
        .into_iter()
        .filter(|line| line.segment > round.last_segment)
        .collect();
    if new_lines.is_empty() {
        round.client = Some((provider, client));
        return Ok(None);
    }
    let bullets = highlights::update(
        &client,
        &Update {
            current: &round.bullets,
            new_lines: &new_lines,
            terms: &terms,
            budget: provider.budget(),
        },
        &Cancel::new(),
    )
    .await?;
    round.bullets = bullets.clone();
    round.last_segment = newest;
    let label = provider.label();
    let meeting = round.key.meeting;
    round.client = Some((provider, client));
    let row = state.engine()?.store().call(move |store| {
        store.save_summary(
            meeting,
            &NewSummary {
                kind: SummaryKind::Highlights,
                template: "highlights",
                provider: &label,
                title: None,
                content: &bullets,
                truncated: false,
            },
        )?;
        store.latest_summary(meeting, SummaryKind::Highlights)
    })?;
    Ok(row.map(SummaryView::from))
}

/// The latest highlights of a meeting, for a window opened during the recording.
#[tauri::command]
pub async fn get_highlights(
    state: State<'_, AppState>,
    meeting: i64,
) -> CommandResult<Option<SummaryView>> {
    Ok(state
        .read(|store| store.latest_summary(meeting, SummaryKind::Highlights))?
        .map(SummaryView::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(meeting: i64, started_at_ms: i64) -> Round {
        Round {
            key: Key {
                meeting,
                started_at_ms,
            },
            bullets: "- old".into(),
            last_segment: 7,
            last_run: Instant::now(),
            client: None,
        }
    }

    #[test]
    fn a_reused_meeting_id_starts_a_new_round() {
        let key = |meeting, started_at_ms| Key {
            meeting,
            started_at_ms,
        };
        assert!(is_new(None, key(3, 1_000)));
        let current = round(3, 1_000);
        assert!(!is_new(Some(&current), key(3, 1_000)));
        // Meeting 3 was deleted and its id given to the next recording.
        assert!(is_new(Some(&current), key(3, 2_000)));
        assert!(is_new(Some(&current), key(4, 1_000)));
    }
}
