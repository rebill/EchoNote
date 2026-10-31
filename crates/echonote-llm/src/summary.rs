//! Meeting notes from a transcript of any length (PRD A11), ported from the plugin's
//! summary planner. A transcript that fits the budget is written in one streamed pass. A
//! longer one is cut at line boundaries into parts, noted two at a time; the notes are
//! merged in rounds until one pass can write the final notes, which streams. A part the
//! server still finds too long is halved and tried again, so a context window smaller than
//! configured costs more requests, not a failure.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use futures_util::{StreamExt, TryStreamExt, stream};

use crate::client::{Chat, Message, Request};
use crate::transcript::{Language, Line, join, sanitize_markdown, split_text, split_title};
use crate::{Cancel, LlmError, prompts, tokens};

/// Parts noted at the same time.
const CONCURRENCY: usize = 2;
/// Share of the context window for the prompt; the rest is the answer and a margin.
const INPUT_SHARE: f64 = 0.7;
/// The `<transcript>` markers and line breaks around the content.
const WRAP_TOKENS: usize = 16;
/// Below this a merge cannot make progress.
const MIN_ROOM: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// The model's context window in tokens.
    pub context_tokens: usize,
    /// Most the final notes may take.
    pub max_output_tokens: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            context_tokens: 32_000,
            max_output_tokens: 4_096,
        }
    }
}

impl Budget {
    /// Tokens for an answer: at most a quarter of the window.
    pub fn output(&self) -> usize {
        self.max_output_tokens.min(self.context_tokens / 4).max(256)
    }

    /// Tokens for one request's prompt.
    pub fn input(&self) -> usize {
        ((self.context_tokens as f64 * INPUT_SHARE) as usize)
            .saturating_sub(self.output())
            .max(1_000)
    }

    /// Most one part's notes may take, so that several fit in a merge.
    fn note(&self) -> usize {
        (self.input() / 5).clamp(300, 1_500)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// `done` of `total` parts are noted.
    Parts { done: usize, total: usize },
    /// Merging `notes` part notes.
    Merging { notes: usize },
    /// The final notes are being written and streamed.
    Writing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub title: Option<String>,
    /// Markdown without the title line, with images and HTML made text.
    pub body: String,
    pub truncated: bool,
    /// Parts the transcript was split into; 1 when it fit.
    pub parts: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum Template<'a> {
    /// One of [`prompts::BUILTIN_TEMPLATES`]; an unknown id is the general template.
    Builtin(&'a str),
    /// The user's own sections and instructions.
    Custom(&'a str),
}

impl Template<'_> {
    fn instructions(&self, language: Language) -> &str {
        match self {
            Template::Builtin(id) => prompts::builtin_instructions(id, language)
                .or_else(|| prompts::builtin_instructions("general", language))
                .unwrap_or_default(),
            Template::Custom(text) => text,
        }
    }
}

pub struct Job<'a> {
    pub lines: &'a [Line],
    pub template: Template<'a>,
    /// Glossary terms, spelled right.
    pub terms: &'a [String],
    pub budget: Budget,
}

fn request(system: &str, content: &str, max_tokens: usize) -> Request {
    Request {
        system: system.to_owned(),
        messages: vec![Message::user(prompts::wrap(content))],
        max_tokens: max_tokens as u32,
        temperature: Some(0.2),
    }
}

fn finish(markdown: &str, truncated: bool, parts: usize) -> Summary {
    let (title, body) = split_title(markdown);
    Summary {
        title,
        body: sanitize_markdown(&body),
        truncated,
        parts,
    }
}
/// 85% of the smallest prompt the server refused as too long: requests are sized below it
/// from then on, so a window smaller than configured is learned in a step or two (the
/// estimates are rough anyway), not one refusal per request.
struct Cap(AtomicUsize);

impl Cap {
    fn get(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }

    fn refused(&self, tokens: usize) {
        self.0.fetch_min(tokens * 85 / 100, Ordering::Relaxed);
    }

    /// Content tokens that fit next to `system` within `budget` and the cap.
    fn room(&self, budget: Budget, system: &str) -> usize {
        budget
            .input()
            .min(self.get())
            .saturating_sub(tokens::estimate(system) + WRAP_TOKENS)
    }
}

/// `request(system, content)`, recording a refusal as too long in `cap`.
async fn ask<C: Chat>(
    chat: &C,
    cap: &Cap,
    system: &str,
    content: &str,
    max_tokens: usize,
    cancel: &Cancel,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<(String, bool), LlmError> {
    let mut text = String::new();
    let result = chat
        .stream(
            &request(system, content, max_tokens),
            cancel,
            &mut |chunk: &str| {
                text.push_str(chunk);
                on_text(chunk);
            },
        )
        .await;
    match result {
        Ok(finish) => Ok((text, finish.truncated)),
        Err(error @ LlmError::ContextTooLong { .. }) => {
            cap.refused(tokens::estimate(system) + tokens::estimate(content) + WRAP_TOKENS);
            Err(error)
        }
        Err(error) => Err(error),
    }
}

/// Merge rounds before giving up (each one at least halves the notes or the cap).
const MAX_ROUNDS: usize = 64;

/// Write notes for `job`. Only the final pass streams to `on_text`.
pub async fn summarize<C: Chat>(
    chat: &C,
    job: &Job<'_>,
    cancel: &Cancel,
    on_progress: &mut (dyn FnMut(Progress) + Send),
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<Summary, LlmError> {
    if job.lines.iter().all(|line| line.text.trim().is_empty()) {
        return Err(LlmError::EmptyTranscript);
    }
    let language = Language::of(job.lines);
    let instructions = job.template.instructions(language);
    let budget = job.budget;
    let cap = Cap(AtomicUsize::new(usize::MAX));
    let cap = &cap;
    let system = prompts::notes(language, instructions, job.terms);
    let transcript = join(job.lines);
    if tokens::estimate(&transcript) <= cap.room(budget, &system) {
        on_progress(Progress::Writing);
        // The server may count more tokens than the estimate: split after all.
        match ask(
            chat,
            cap,
            &system,
            &transcript,
            budget.output(),
            cancel,
            on_text,
        )
        .await
        {
            Err(LlmError::ContextTooLong { .. }) => {}
            result => return result.map(|(text, truncated)| finish(&text, truncated, 1)),
        }
    }

    let partial = prompts::partial(language, 999, 999, job.terms);
    let parts = chunks(job.lines, cap.room(budget, &partial).max(MIN_ROOM));
    let total = parts.len();
    on_progress(Progress::Parts { done: 0, total });
    let noted: Vec<Vec<String>> = {
        let shared = Mutex::new((&mut *on_progress, 0usize));
        let shared = &shared;
        stream::iter(parts.into_iter().enumerate())
            .map(|(index, part)| async move {
                let system = prompts::partial(language, index + 1, total, job.terms);
                let notes = note_part(chat, cap, &system, part, budget, cancel).await?;
                let mut guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
                let (report, done) = &mut *guard;
                *done += 1;
                report(Progress::Parts { done: *done, total });
                Ok::<_, LlmError>(notes)
            })
            .buffered(CONCURRENCY)
            .try_collect()
            .await?
    };
    let mut notes: Vec<String> = noted.into_iter().flatten().collect();

    let final_system = prompts::notes_from_parts(language, instructions, job.terms);
    let merge_system = prompts::merge(language);
    for _ in 0..MAX_ROUNDS {
        let all = number(&notes);
        if tokens::estimate(&all) <= cap.room(budget, &final_system) {
            on_progress(Progress::Writing);
            match ask(
                chat,
                cap,
                &final_system,
                &all,
                budget.output(),
                cancel,
                on_text,
            )
            .await
            {
                Err(LlmError::ContextTooLong { .. }) => {}
                result => return result.map(|(text, truncated)| finish(&text, truncated, total)),
            }
        }
        let room = cap.room(budget, merge_system);
        if room < MIN_ROOM {
            break;
        }
        // A note too big for a merge on its own is cut first.
        let pieces: Vec<String> = notes
            .iter()
            .flat_map(|note| {
                if tokens::estimate(note) + 4 > room {
                    split_text(note, room / 2)
                } else {
                    vec![note.clone()]
                }
            })
            .collect();
        let groups = group(&pieces, room);
        if groups.len() >= pieces.len() && pieces.len() > 1 {
            break;
        }
        on_progress(Progress::Merging {
            notes: pieces.len(),
        });
        let max_tokens = note_tokens(budget, room);
        let merged = stream::iter(groups)
            .map(|group| async move {
                let (text, _) = ask(
                    chat,
                    cap,
                    merge_system,
                    &number(&group),
                    max_tokens,
                    cancel,
                    &mut |_| {},
                )
                .await?;
                Ok::<_, LlmError>(text.trim().to_owned())
            })
            .buffered(CONCURRENCY)
            .try_collect()
            .await;
        match merged {
            Ok(merged) => notes = merged,
            // The cap is lower now; regroup.
            Err(LlmError::ContextTooLong { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    Err(LlmError::ContextTooLong {
        detail: "the notes do not fit this model's context window".into(),
    })
}

/// Most a note may take: small enough that three fit in a merge of `room` tokens.
fn note_tokens(budget: Budget, room: usize) -> usize {
    budget.note().min(room / 3).max(128)
}

/// Notes on one part, one per piece: a piece the server finds too long is halved.
async fn note_part<C: Chat>(
    chat: &C,
    cap: &Cap,
    system: &str,
    part: Vec<String>,
    budget: Budget,
    cancel: &Cancel,
) -> Result<Vec<String>, LlmError> {
    let mut queue = vec![part];
    let mut notes = Vec::new();
    while let Some(mut lines) = queue.pop() {
        let room = cap.room(budget, system);
        let content = lines.join("\n");
        // Known not to fit since an earlier refusal: split without asking.
        if tokens::estimate(&content) > room && lines.len() > 1 {
            let second = lines.split_off(lines.len() / 2);
            queue.push(second);
            queue.push(lines);
            continue;
        }
        match ask(
            chat,
            cap,
            system,
            &content,
            note_tokens(budget, room),
            cancel,
            &mut |_| {},
        )
        .await
        {
            Ok((text, _)) => notes.push(text.trim().to_owned()),
            Err(LlmError::ContextTooLong { .. }) if lines.len() > 1 => {
                let second = lines.split_off(lines.len() / 2);
                queue.push(second);
                queue.push(lines);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(notes)
}

/// Line texts in parts of at most `room` tokens; a longer line is cut.
fn chunks(lines: &[Line], room: usize) -> Vec<Vec<String>> {
    let mut parts = Vec::new();
    let mut part: Vec<String> = Vec::new();
    let mut size = 0;
    for line in lines.iter().filter(|line| !line.text.trim().is_empty()) {
        let pieces = if tokens::estimate(&line.text) + 1 > room {
            // A token is at least one character, so `room` characters fit.
            split_text(&line.text, room)
        } else {
            vec![line.text.clone()]
        };
        for piece in pieces {
            let cost = tokens::estimate(&piece) + 1;
            if !part.is_empty() && size + cost > room {
                parts.push(std::mem::take(&mut part));
                size = 0;
            }
            part.push(piece);
            size += cost;
        }
    }
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}

/// Consecutive notes in groups of at most `room` tokens.
fn group(notes: &[String], room: usize) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut size = 0;
    for note in notes {
        let cost = tokens::estimate(note) + 4;
        match groups.last_mut() {
            Some(last) if size + cost <= room => last.push(note.clone()),
            _ => {
                groups.push(vec![note.clone()]);
                size = 0;
            }
        }
        size += cost;
    }
    groups
}

fn number(notes: &[String]) -> String {
    notes
        .iter()
        .enumerate()
        .map(|(index, note)| format!("--- {} ---\n{note}", index + 1))
        .collect::<Vec<_>>()
        .join("\n\n")
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Asked, FakeChat};

    fn run<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn lines(count: usize) -> Vec<Line> {
        (0..count)
            .map(|i| Line {
                segment: i as i64 + 1,
                start_ms: i as i64 * 5_000,
                text: format!(
                    "[{:02}:{:02}] 我：第{i}句，我们讨论预算和上线时间，需要再确认一下",
                    i * 5 / 60,
                    i * 5 % 60
                ),
            })
            .collect()
    }

    /// Short notes for parts and merges, a titled summary for the final pass.
    fn responder(asked: &Asked) -> Result<String, LlmError> {
        Ok(if asked.system.contains("部分。") {
            "- 讨论预算和上线时间 [00:05]\n".repeat(8)
        } else if asked.system.starts_with("下面是一场会议连续几部分的笔记") {
            "- 合并：预算 [00:05]".into()
        } else {
            "# 预算会议\n\n## 概要\n讨论了预算。".into()
        })
    }

    fn job<'a>(lines: &'a [Line], terms: &'a [String], context_tokens: usize) -> Job<'a> {
        Job {
            lines,
            template: Template::Builtin("general"),
            terms,
            budget: Budget {
                context_tokens,
                max_output_tokens: 4_096,
            },
        }
    }

    fn summarize_with(
        chat: &FakeChat,
        job: &Job<'_>,
    ) -> (Result<Summary, LlmError>, Vec<Progress>, String) {
        let mut progress = Vec::new();
        let mut text = String::new();
        let result = run(summarize(
            chat,
            job,
            &Cancel::new(),
            &mut |p| progress.push(p),
            &mut |chunk| text.push_str(chunk),
        ));
        (result, progress, text)
    }

    #[test]
    fn a_short_meeting_is_written_in_one_streamed_pass() {
        let chat = FakeChat::new(100_000, responder);
        let lines = lines(20);
        let terms = vec!["飞书".to_owned()];
        let (result, progress, text) = summarize_with(&chat, &job(&lines, &terms, 32_000));
        let summary = result.unwrap();
        assert_eq!(summary.title.as_deref(), Some("预算会议"));
        assert_eq!(summary.body, "## 概要\n讨论了预算。");
        assert_eq!(summary.parts, 1);
        assert!(text.starts_with("# 预算会议"));
        assert_eq!(progress, [Progress::Writing]);
        let asked = chat.asked();
        assert_eq!(asked.len(), 1);
        assert!(asked[0].system.contains("飞书") && asked[0].system.contains("## 待办"));
        assert!(asked[0].user[0].starts_with("<transcript>\n[00:00] 我：第0句"));
        assert_eq!(asked[0].max_tokens, 4_096);
    }

    #[test]
    fn a_long_meeting_is_split_noted_and_merged_within_the_budget() {
        let budget = Budget {
            context_tokens: 4_000,
            max_output_tokens: 4_096,
        };
        let chat = FakeChat::new(budget.input(), responder);
        let lines = lines(2_000);
        let (result, progress, text) =
            summarize_with(&chat, &job(&lines, &[], budget.context_tokens));
        let summary = result.unwrap();
        assert_eq!(summary.title.as_deref(), Some("预算会议"));
        assert!(summary.parts > 10, "{}", summary.parts);
        assert!(
            text.starts_with("# 预算会议"),
            "only the final pass streams"
        );
        let asked = chat.asked();
        assert!(asked.iter().all(|a| a.tokens <= budget.input()));
        assert_eq!(
            asked.iter().filter(|a| a.system.contains("部分。")).count(),
            summary.parts
        );
        assert!(
            asked
                .last()
                .unwrap()
                .system
                .starts_with("转录太长，已经分段做了笔记")
        );
        let parts: Vec<_> = progress
            .iter()
            .filter_map(|p| match p {
                Progress::Parts { done, total } => Some((*done, *total)),
                _ => None,
            })
            .collect();
        assert_eq!(parts.first(), Some(&(0, summary.parts)));
        assert_eq!(parts.last(), Some(&(summary.parts, summary.parts)));
        assert!(
            progress
                .iter()
                .any(|p| matches!(p, Progress::Merging { .. }))
        );
        assert!(
            asked
                .iter()
                .any(|a| a.system.starts_with("下面是一场会议连续几部分的笔记"))
        );
        assert_eq!(progress.last(), Some(&Progress::Writing));
    }

    #[test]
    fn a_server_with_a_smaller_window_than_configured_gets_smaller_parts() {
        // Configured for 32k, but the server takes 1500 tokens.
        let chat = FakeChat::new(1_500, responder);
        let lines = lines(400);
        let (result, _, _) = summarize_with(&chat, &job(&lines, &[], 32_000));
        let summary = result.unwrap();
        assert_eq!(summary.title.as_deref(), Some("预算会议"));
        let asked = chat.asked();
        let refused = asked.iter().filter(|a| a.tokens > 1_500).count();
        assert!(
            refused > 0 && asked.len() > refused + 2,
            "{} of {}",
            refused,
            asked.len()
        );
        // Learned in a few refusals (15 requests, 4 refused when written).
        assert!(
            refused <= 8 && asked.len() <= 30,
            "{} of {}",
            refused,
            asked.len()
        );
    }

    #[test]
    fn cancel_and_an_empty_transcript_stop_early() {
        let chat = FakeChat::new(100_000, responder);
        let lines = lines(5);
        let cancel = Cancel::new();
        cancel.cancel();
        let result = run(summarize(
            &chat,
            &job(&lines, &[], 32_000),
            &cancel,
            &mut |_| {},
            &mut |_| {},
        ));
        assert!(matches!(result, Err(LlmError::Cancelled)));
        let empty = [Line {
            segment: 1,
            start_ms: 0,
            text: " ".into(),
        }];
        let (result, _, _) = summarize_with(&chat, &job(&empty, &[], 32_000));
        assert!(matches!(result, Err(LlmError::EmptyTranscript)));
        assert_eq!(chat.asked().len(), 1);
    }

    #[test]
    fn budgets_leave_room_for_the_answer() {
        let small = Budget {
            context_tokens: 8_192,
            max_output_tokens: 4_096,
        };
        assert_eq!(small.output(), 2_048);
        assert!(small.input() + small.output() <= 8_192);
        let large = Budget {
            context_tokens: 128_000,
            max_output_tokens: 4_096,
        };
        assert_eq!(large.output(), 4_096);
        assert_eq!(large.input(), 89_600 - 4_096);
    }
}
