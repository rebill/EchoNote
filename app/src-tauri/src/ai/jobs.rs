//! Notes, Q&A and correction runs. Each streams over the `ai` event and can be cancelled;
//! one of each kind runs per meeting. The setup (provider, consent, key, transcript) is
//! checked before the command returns, so those errors come back to the click.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use echonote_core::engine::SessionState;
use echonote_core::export::{self, Export};
use echonote_core::store::{
    Crosstalk, MeetingStatus, NewCorrection, NewSummary, QaTurn, SummaryKind, SummaryRow,
};
use echonote_llm::summary::{self, Template};
use echonote_llm::transcript::Line;
use echonote_llm::{Cancel, Client, LlmError, Policy, correct, qa};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::ai::config::{Provider, valid_template};
use crate::commands::MAX_TITLE;
use crate::error::{CommandError, CommandResult};
use crate::log::log;
use crate::state::{AppState, lock};

/// How often streamed text is sent to the UI.
const FLUSH: Duration = Duration::from_millis(50);
const MAX_QUESTION: usize = 1_000;
/// Earlier turns sent with a question.
const HISTORY: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Notes,
    Answer,
    Correct,
    Highlights,
    /// The high-accuracy model (not an LLM run, but cancelled and shown the same way).
    Retranscribe,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Notes => "notes",
            Kind::Answer => "answer",
            Kind::Correct => "correct",
            Kind::Highlights => "highlights",
            Kind::Retranscribe => "retranscribe",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum Body {
    /// More streamed text.
    Text {
        text: String,
    },
    Progress(Stage),
    Done {
        result: serde_json::Value,
    },
    Error {
        error: CommandError,
    },
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "stage", rename_all = "lowercase")]
pub enum Stage {
    Parts { done: usize, total: usize },
    Merging { notes: usize },
    Writing,
    Batches { done: usize, total: usize },
    Segments { done: usize, total: usize },
}

/// One `ai` event. Job 0 is live highlights.
#[derive(Clone, Debug, Serialize)]
pub struct AiEvent {
    pub job: u64,
    pub meeting: i64,
    pub kind: Kind,
    #[serde(flatten)]
    pub body: Body,
}

struct Running {
    meeting: i64,
    kind: Kind,
    cancel: Cancel,
}

#[derive(Default)]
pub struct Jobs {
    next: AtomicU64,
    running: Mutex<HashMap<u64, Running>>,
}

impl Jobs {
    fn start(&self, meeting: i64, kind: Kind) -> CommandResult<(u64, Cancel)> {
        let mut running = lock(&self.running);
        if running
            .values()
            .any(|job| job.meeting == meeting && job.kind == kind)
        {
            return Err(CommandError::new("job-running", "this is already running"));
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = Cancel::new();
        running.insert(
            id,
            Running {
                meeting,
                kind,
                cancel: cancel.clone(),
            },
        );
        Ok((id, cancel))
    }

    fn finish(&self, id: u64) {
        lock(&self.running).remove(&id);
    }

    pub fn cancel(&self, id: u64) {
        if let Some(job) = lock(&self.running).get(&id) {
            job.cancel.cancel();
        }
    }

    pub fn cancel_meeting(&self, meeting: i64) {
        for job in lock(&self.running).values() {
            if job.meeting == meeting {
                job.cancel.cancel();
            }
        }
    }

    /// Runs in progress: (job, meeting, kind).
    pub fn list(&self) -> Vec<(u64, i64, Kind)> {
        lock(&self.running)
            .iter()
            .map(|(id, job)| (*id, job.meeting, job.kind))
            .collect()
    }
}

/// Where a run reports.
pub struct Out {
    pub app: AppHandle,
    pub job: u64,
    pub meeting: i64,
    pub kind: Kind,
}

impl Out {
    pub fn send(&self, body: Body) {
        let _ = self.app.emit(
            "ai",
            AiEvent {
                job: self.job,
                meeting: self.meeting,
                kind: self.kind,
                body,
            },
        );
    }

    pub fn progress(&self, stage: Stage) {
        self.send(Body::Progress(stage));
    }

    /// Write to the library through the store thread.
    fn write<T: Send + 'static>(
        &self,
        task: impl FnOnce(
            &mut echonote_core::store::Store,
        ) -> Result<T, echonote_core::store::StoreError>
        + Send
        + 'static,
    ) -> CommandResult<T> {
        Ok(self.app.state::<AppState>().engine()?.store().call(task)?)
    }
}

/// Streamed text, sent at most every [`FLUSH`].
pub struct Stream<'a> {
    out: &'a Out,
    buffer: String,
    last: Option<Instant>,
}

impl<'a> Stream<'a> {
    pub fn new(out: &'a Out) -> Self {
        Self {
            out,
            buffer: String::new(),
            last: None,
        }
    }

    pub fn push(&mut self, text: &str) {
        self.buffer.push_str(text);
        if self.last.is_none_or(|last| last.elapsed() >= FLUSH) {
            self.flush();
        }
    }

    pub fn flush(&mut self) {
        if !self.buffer.is_empty() {
            let text = std::mem::take(&mut self.buffer);
            self.out.send(Body::Text { text });
            self.last = Some(Instant::now());
        }
    }
}

/// Marks the run finished even if it panics.
struct Finish<'a>(&'a Jobs, u64);

impl Drop for Finish<'_> {
    fn drop(&mut self) {
        self.0.finish(self.1);
    }
}

/// Start `work` as a run of `kind`; returns its job id at once.
pub fn spawn<F>(
    app: &AppHandle,
    meeting: i64,
    kind: Kind,
    work: impl FnOnce(Out, Cancel) -> F,
) -> CommandResult<u64>
where
    F: Future<Output = (Out, CommandResult<serde_json::Value>)> + Send + 'static,
{
    let state = app.state::<AppState>();
    let (job, cancel) = state.jobs.start(meeting, kind)?;
    let out = Out {
        app: app.clone(),
        job,
        meeting,
        kind,
    };
    let future = work(out, cancel);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let finish = Finish(&state.jobs, job);
        let (out, result) = future.await;
        drop(finish);
        match result {
            Ok(result) => {
                log!("{} done", kind.as_str());
                out.send(Body::Done { result });
            }
            Err(error) => {
                log!("{} failed: {}", kind.as_str(), error.code);
                out.send(Body::Error { error });
            }
        }
    });
    Ok(job)
}

/// The active provider with a client, or why a run cannot start.
pub struct Ready {
    pub provider: Provider,
    pub client: Client,
}

pub fn ready(state: &AppState, policy: Policy) -> CommandResult<Ready> {
    let provider = state.ai().active()?.clone();
    if provider.needs_consent() {
        return Err(CommandError::new(
            "consent-required",
            provider.origin().unwrap_or_default(),
        ));
    }
    let secret = if provider.has_key || !provider.header_names.is_empty() {
        state.secrets.get(&provider.id)?
    } else {
        Default::default()
    };
    if (provider.has_key && secret.api_key.is_none())
        || secret.headers.len() < provider.header_names.len()
    {
        return Err(CommandError::new(
            "key-missing",
            "the saved key is no longer in the keychain",
        ));
    }
    let client = provider.client(secret, policy)?;
    Ok(Ready { provider, client })
}

/// Refuse a meeting that is being recorded or still needs recovering.
pub fn idle(state: &AppState, meeting: i64) -> CommandResult<()> {
    let live = state
        .engine()?
        .state()
        .is_some_and(|(id, session)| id == meeting && session != SessionState::Finalized);
    let status = state.read(|store| store.meeting(meeting))?.status;
    if live || status == MeetingStatus::Recording {
        return Err(CommandError::new(
            "meeting-in-use",
            "the meeting is being recorded",
        ));
    }
    Ok(())
}

fn export_language(state: &AppState) -> export::Language {
    match state.settings().language.as_str() {
        "en" => export::Language::En,
        _ => export::Language::Zh,
    }
}

/// The transcript as the model reads it (glossary applied, hidden lines left out) and the
/// glossary's terms.
pub fn transcript(state: &AppState, meeting: i64) -> CommandResult<(Vec<Line>, Vec<String>)> {
    let glossary = state.glossary();
    let (row, segments) =
        state.read(|store| Ok((store.meeting(meeting)?, store.segments(meeting)?)))?;
    let export = Export {
        meeting: &row,
        segments: &segments,
        glossary: &glossary,
        language: export_language(state),
        utc_offset_minutes: 0,
        notes: None,
    };
    let lines = export
        .transcript()
        .into_iter()
        .map(|line| Line {
            segment: line.segment,
            start_ms: line.start_ms,
            text: line.text,
        })
        .collect();
    Ok((lines, glossary.terms().to_vec()))
}

fn need_lines(lines: &[Line]) -> CommandResult<()> {
    if lines.is_empty() {
        return Err(LlmError::EmptyTranscript.into());
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryView {
    id: i64,
    template: String,
    provider: String,
    title: Option<String>,
    content: String,
    truncated: bool,
    created_at_ms: i64,
}

impl From<SummaryRow> for SummaryView {
    fn from(row: SummaryRow) -> Self {
        Self {
            id: row.id,
            template: row.template,
            provider: row.provider,
            title: row.title,
            content: row.content,
            truncated: row.truncated,
            created_at_ms: row.created_at_ms,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QaView {
    id: i64,
    question: String,
    answer: String,
    provider: String,
    created_at_ms: i64,
    /// The answer hit the output limit.
    truncated: bool,
    /// Only the most relevant parts of a long transcript were read.
    excerpt: bool,
}

impl From<QaTurn> for QaView {
    fn from(turn: QaTurn) -> Self {
        Self {
            id: turn.id,
            question: turn.question,
            answer: turn.answer,
            provider: turn.provider,
            created_at_ms: turn.created_at_ms,
            truncated: false,
            excerpt: false,
        }
    }
}

fn json(value: impl Serialize) -> CommandResult<serde_json::Value> {
    serde_json::to_value(value).map_err(|error| CommandError::new("engine", error.to_string()))
}

/// A model's title, made safe for the meeting list and file names.
fn clean_title(title: &str) -> String {
    let title: String = title
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    title
        .trim()
        .chars()
        .take(MAX_TITLE)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Write notes for a meeting with `template` (a built-in id or `custom:<id>`, kept as the
/// default). An untitled meeting takes the notes' title. Returns the job id.
#[tauri::command]
pub async fn summarize(app: AppHandle, meeting: i64, template: String) -> CommandResult<u64> {
    if !valid_template(&template) {
        return Err(CommandError::invalid("unknown template"));
    }
    let handle = app.clone();
    crate::commands::blocking(app, move |state| {
        idle(state, meeting)?;
        let Ready { provider, client } = ready(state, Policy::default())?;
        let (lines, terms) = transcript(state, meeting)?;
        need_lines(&lines)?;
        let custom = match template.strip_prefix("custom:") {
            Some(id) => {
                let id = id
                    .parse()
                    .map_err(|_| CommandError::invalid("unknown template"))?;
                Some(state.read(|store| store.template(id))?.instructions)
            }
            None => None,
        };
        let chosen = template.clone();
        state.update_ai(move |settings| {
            settings.template = chosen;
            Ok(())
        })?;
        spawn(
            &handle,
            meeting,
            Kind::Notes,
            move |out, cancel| async move {
                let result = async {
                    let job = summary::Job {
                        lines: &lines,
                        template: match &custom {
                            Some(instructions) => Template::Custom(instructions),
                            None => Template::Builtin(&template),
                        },
                        terms: &terms,
                        budget: provider.budget(),
                    };
                    let mut stream = Stream::new(&out);
                    let summary = summary::summarize(
                        &client,
                        &job,
                        &cancel,
                        &mut |progress| {
                            out.progress(match progress {
                                summary::Progress::Parts { done, total } => {
                                    Stage::Parts { done, total }
                                }
                                summary::Progress::Merging { notes } => Stage::Merging { notes },
                                summary::Progress::Writing => Stage::Writing,
                            })
                        },
                        &mut |text| stream.push(text),
                    )
                    .await;
                    stream.flush();
                    let summary = summary?;
                    let label = provider.label();
                    let title = summary
                        .title
                        .as_deref()
                        .map(clean_title)
                        .filter(|t| !t.is_empty());
                    let (row, renamed) = out.write(move |store| {
                        store.save_summary(
                            meeting,
                            &NewSummary {
                                kind: SummaryKind::Notes,
                                template: &template,
                                provider: &label,
                                title: title.as_deref(),
                                content: &summary.body,
                                truncated: summary.truncated,
                            },
                        )?;
                        // Only a meeting the user has not named (they may have meanwhile).
                        let renamed = match &title {
                            Some(title) if store.meeting(meeting)?.title.trim().is_empty() => {
                                store.rename_meeting(meeting, title)?;
                                true
                            }
                            _ => false,
                        };
                        let row = store
                            .latest_summary(meeting, SummaryKind::Notes)?
                            .expect("just saved");
                        Ok((row, renamed))
                    })?;
                    if renamed {
                        let _ = out.app.emit("library-changed", ());
                    }
                    json(SummaryView::from(row))
                }
                .await;
                (out, result)
            },
        )
    })
    .await
}

/// Ask about a meeting; the answer cites `[mm:ss]` times. Returns the job id.
#[tauri::command]
pub async fn ask(app: AppHandle, meeting: i64, question: String) -> CommandResult<u64> {
    let question = question.trim().to_owned();
    if question.is_empty() || question.chars().count() > MAX_QUESTION {
        return Err(CommandError::invalid("the question is empty or too long"));
    }
    let handle = app.clone();
    crate::commands::blocking(app, move |state| {
        idle(state, meeting)?;
        let Ready { provider, client } = ready(state, Policy::default())?;
        let (lines, terms) = transcript(state, meeting)?;
        need_lines(&lines)?;
        let (turns, notes) = state.read(|store| {
            Ok((
                store.qa(meeting)?,
                store.latest_summary(meeting, SummaryKind::Notes)?,
            ))
        })?;
        let history: Vec<qa::Turn> = turns
            .into_iter()
            .rev()
            .take(HISTORY)
            .rev()
            .map(|turn| qa::Turn {
                question: turn.question,
                answer: turn.answer,
            })
            .collect();
        let notes = notes.map(|notes| notes.content);
        spawn(
            &handle,
            meeting,
            Kind::Answer,
            move |out, cancel| async move {
                let result = async {
                    let ask = qa::Ask {
                        lines: &lines,
                        question: &question,
                        history: &history,
                        notes: notes.as_deref(),
                        terms: &terms,
                        budget: provider.budget(),
                    };
                    let mut stream = Stream::new(&out);
                    let answer =
                        qa::answer(&client, &ask, &cancel, &mut |text| stream.push(text)).await;
                    stream.flush();
                    let answer = answer?;
                    let label = provider.label();
                    let text = answer.text.clone();
                    let turn = out.write(move |store| {
                        let id = store.add_qa(meeting, &question, &text, &label)?;
                        let turn = store.qa(meeting)?.into_iter().find(|turn| turn.id == id);
                        Ok(turn.expect("just saved"))
                    })?;
                    json(QaView {
                        truncated: answer.truncated,
                        excerpt: answer.excerpt,
                        ..QaView::from(turn)
                    })
                }
                .await;
                (out, result)
            },
        )
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CorrectView {
    applied: usize,
    rejected: usize,
    failed_batches: usize,
    batches: usize,
}

/// Let the model fix misheard names and words. Changes are written as revisions and wait
/// for review (keep or revert). Returns the job id.
#[tauri::command]
pub async fn correct_meeting(app: AppHandle, meeting: i64) -> CommandResult<u64> {
    let handle = app.clone();
    crate::commands::blocking(app, move |state| {
        idle(state, meeting)?;
        let Ready { provider, client } = ready(state, Policy::default())?;
        let glossary = state.glossary();
        let segments = state.read(|store| store.segments(meeting))?;
        // The text as shown, so the glossary's own replacements are not counted as changes.
        let mut sources = Vec::new();
        let mut revisions = HashMap::new();
        for segment in &segments {
            if segment.crosstalk == Some(Crosstalk::Duplicate) {
                continue;
            }
            let text = glossary.apply(&segment.text).trim().to_owned();
            if text.is_empty() {
                continue;
            }
            revisions.insert(segment.id, segment.revision);
            sources.push(correct::Source {
                segment: segment.id,
                text,
            });
        }
        if sources.is_empty() {
            return Err(LlmError::EmptyTranscript.into());
        }
        let terms = glossary.terms().to_vec();
        spawn(
            &handle,
            meeting,
            Kind::Correct,
            move |out, cancel| async move {
                let result = async {
                    let outcome = correct::correct(
                        &client,
                        &sources,
                        &terms,
                        provider.budget(),
                        &cancel,
                        &mut |done, total| out.progress(Stage::Batches { done, total }),
                    )
                    .await?;
                    let corrections: Vec<NewCorrection> = outcome
                        .fixes
                        .iter()
                        .filter_map(|fix| {
                            Some(NewCorrection {
                                segment: fix.segment,
                                revision: *revisions.get(&fix.segment)?,
                                text: fix.text.clone(),
                            })
                        })
                        .collect();
                    // The run can end after the meeting was deleted; then nothing is written.
                    let applied = if corrections.is_empty() {
                        0
                    } else {
                        out.write(move |store| store.apply_corrections(meeting, &corrections))?
                    };
                    if applied > 0 {
                        let _ = out.app.emit("library-changed", ());
                    }
                    json(CorrectView {
                        applied,
                        rejected: outcome.rejected,
                        failed_batches: outcome.failed_batches,
                        batches: outcome.batches,
                    })
                }
                .await;
                (out, result)
            },
        )
    })
    .await
}

/// Keep the model's changes: one segment's, or the whole meeting's.
#[tauri::command]
pub async fn keep_corrections(
    state: State<'_, AppState>,
    meeting: i64,
    segment: Option<i64>,
) -> CommandResult<()> {
    state
        .engine()?
        .store()
        .call(move |store| store.keep_corrections(meeting, segment))?;
    Ok(())
}

/// Undo the model's change to a segment; returns the segment as shown.
#[tauri::command]
pub async fn revert_correction(
    app: AppHandle,
    state: State<'_, AppState>,
    segment: i64,
) -> CommandResult<crate::views::SegmentView> {
    state
        .engine()?
        .store()
        .call(move |store| store.revert_correction(segment))?;
    let _ = app.emit("library-changed", ());
    crate::edit::shown(&state, segment)
}

#[tauri::command]
pub async fn clear_qa(state: State<'_, AppState>, meeting: i64) -> CommandResult<()> {
    state
        .engine()?
        .store()
        .call(move |store| store.clear_qa(meeting))?;
    Ok(())
}

#[tauri::command]
pub async fn cancel_job(state: State<'_, AppState>, job: u64) -> CommandResult<()> {
    state.jobs.cancel(job);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    job: u64,
    meeting: i64,
    kind: Kind,
}

/// Runs in progress, for a window opened while one runs.
#[tauri::command]
pub async fn running_jobs(state: State<'_, AppState>) -> CommandResult<Vec<JobView>> {
    Ok(state
        .jobs
        .list()
        .into_iter()
        .map(|(job, meeting, kind)| JobView { job, meeting, kind })
        .collect())
}
