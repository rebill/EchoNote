//! Post-meeting correction of names, terms and homophones, ported from the plugin's v0.6
//! correction. Lines go out in JSON batches with short ids; the model returns only the lines
//! it changed, and each change is checked strictly (known id, one line, not empty, length
//! close to the original) before it is offered. A batch that fails or comes back unreadable
//! is skipped; the rest still run. Nothing is written here: the caller stores accepted
//! changes as revisions the user can revert.

use std::collections::HashSet;

use futures_util::{StreamExt, TryStreamExt, stream};
use serde_json::Value;

use crate::client::{Chat, Message, Request};
use crate::summary::Budget;
use crate::transcript::Language;
use crate::{Cancel, LlmError, complete, prompts, tokens};

const CONCURRENCY: usize = 2;
/// Below this many characters a change may be up to three times as long (punctuation, a
/// name spelled out); above, it must stay within these ratios.
const RATIO_CHECK_CHARS: usize = 20;
const MIN_RATIO: f64 = 0.4;
const MAX_RATIO: f64 = 1.8;

/// A line to check: its segment and the text as shown (glossary applied).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub segment: i64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    pub segment: i64,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub fixes: Vec<Fix>,
    /// Changes the checks refused.
    pub rejected: usize,
    /// Batches skipped because the request failed or the answer was unreadable.
    pub failed_batches: usize,
    pub batches: usize,
}

/// Errors that end the whole job rather than one batch.
fn is_fatal(error: &LlmError) -> bool {
    matches!(
        error,
        LlmError::Cancelled
            | LlmError::Unauthorized { .. }
            | LlmError::Quota { .. }
            | LlmError::NotFound { .. }
            | LlmError::Connect
            | LlmError::InvalidUrl
            | LlmError::InsecureUrl
            | LlmError::InvalidProxy
            | LlmError::InvalidHeader
            | LlmError::Setup
            | LlmError::Redirected
    )
}

pub async fn correct<C: Chat>(
    chat: &C,
    lines: &[Source],
    terms: &[String],
    budget: Budget,
    cancel: &Cancel,
    on_progress: &mut (dyn FnMut(usize, usize) + Send),
) -> Result<Outcome, LlmError> {
    let language = Language::of(
        &lines
            .iter()
            .map(|line| crate::transcript::Line {
                segment: line.segment,
                start_ms: 0,
                text: line.text.clone(),
            })
            .collect::<Vec<_>>(),
    );
    let system = prompts::correct(language, terms);
    // The answer can repeat a whole batch, so input and output share the budget.
    let room = (budget.input().saturating_sub(tokens::estimate(&system)) / 2)
        .min(budget.output() * 3 / 4)
        .max(200);
    let batches = batch(lines, room);
    let total = batches.len();
    on_progress(0, total);
    let done = std::sync::Mutex::new((on_progress, 0usize));
    let done = &done;
    let system = &system;
    let results: Vec<(Vec<Fix>, usize, bool)> = stream::iter(batches)
        .map(|batch| async move {
            let result = run_batch(chat, system, &batch, budget, cancel).await;
            let mut guard = done
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (report, count) = &mut *guard;
            *count += 1;
            report(*count, total);
            match result {
                Ok((fixes, rejected)) => Ok((fixes, rejected, false)),
                Err(error) if is_fatal(&error) => Err(error),
                Err(_) => Ok((Vec::new(), 0, true)),
            }
        })
        .buffered(CONCURRENCY)
        .try_collect()
        .await?;
    let mut outcome = Outcome {
        batches: total,
        ..Outcome::default()
    };
    for (fixes, rejected, failed) in results {
        outcome.fixes.extend(fixes);
        outcome.rejected += rejected;
        outcome.failed_batches += usize::from(failed);
    }
    Ok(outcome)
}

/// Consecutive lines in batches of at most `room` tokens of JSON.
fn batch(lines: &[Source], room: usize) -> Vec<Vec<Source>> {
    let mut batches: Vec<Vec<Source>> = Vec::new();
    let mut size = 0;
    for line in lines.iter().filter(|line| !line.text.trim().is_empty()) {
        let cost = tokens::estimate(&line.text) + 8;
        match batches.last_mut() {
            Some(last) if size + cost <= room => last.push(line.clone()),
            _ => {
                batches.push(vec![line.clone()]);
                size = 0;
            }
        }
        size += cost;
    }
    batches
}

async fn run_batch<C: Chat>(
    chat: &C,
    system: &str,
    batch: &[Source],
    budget: Budget,
    cancel: &Cancel,
) -> Result<(Vec<Fix>, usize), LlmError> {
    // Ids 1..n within the batch: short, and no database ids leave the app.
    let input: Vec<Value> = batch
        .iter()
        .enumerate()
        .map(|(index, line)| serde_json::json!({"id": index + 1, "text": line.text}))
        .collect();
    let content = serde_json::json!({ "lines": input }).to_string();
    let request = Request {
        system: system.to_owned(),
        messages: vec![Message::user(prompts::wrap(&content))],
        max_tokens: budget.output() as u32,
        temperature: Some(0.0),
    };
    let (text, _) = complete(chat, &request, cancel).await?;
    let changes = parse(&text).ok_or(LlmError::BadResponse("the corrections are not JSON"))?;
    Ok(validate(batch, &changes))
}

/// `{"lines":[{"id":..,"text":..}]}` from the answer, fenced or with chatter around it.
fn parse(text: &str) -> Option<Vec<(Value, Value)>> {
    let trimmed = text.trim();
    let json = trimmed
        .split_once("```")
        .and_then(|(_, rest)| rest.split_once("```"))
        .map(|(inside, _)| inside.trim_start_matches("json").trim())
        .unwrap_or(trimmed);
    let json = match (json.find('{'), json.rfind('}')) {
        (Some(start), Some(end)) if start < end => &json[start..=end],
        _ => json,
    };
    let value: Value = serde_json::from_str(json).ok()?;
    let lines = value.get("lines")?.as_array()?;
    Some(
        lines
            .iter()
            .map(|line| {
                (
                    line.get("id").cloned().unwrap_or(Value::Null),
                    line.get("text").cloned().unwrap_or(Value::Null),
                )
            })
            .collect(),
    )
}

fn is_safe(original: &str, corrected: &str) -> bool {
    let (before, after) = (original.chars().count(), corrected.chars().count());
    if after == 0 || corrected.contains(['\n', '\r']) {
        return false;
    }
    if before < RATIO_CHECK_CHARS {
        return after <= (before * 3).max(40);
    }
    let ratio = after as f64 / before as f64;
    (MIN_RATIO..=MAX_RATIO).contains(&ratio)
}

/// Accepted fixes and the number of refused changes.
fn validate(batch: &[Source], changes: &[(Value, Value)]) -> (Vec<Fix>, usize) {
    let mut fixes = Vec::new();
    let mut rejected = 0;
    let mut seen = HashSet::new();
    for (id, text) in changes {
        let index = id
            .as_u64()
            .or_else(|| id.as_str().and_then(|id| id.trim().parse().ok()))
            .and_then(|id| usize::try_from(id).ok())
            .filter(|id| (1..=batch.len()).contains(id));
        let (Some(index), Some(text)) = (index, text.as_str()) else {
            rejected += 1;
            continue;
        };
        let original = &batch[index - 1];
        let text = text.trim();
        if text == original.text.trim() {
            continue;
        }
        if !seen.insert(index) || !is_safe(&original.text, text) {
            rejected += 1;
            continue;
        }
        fixes.push(Fix {
            segment: original.segment,
            text: text.to_owned(),
        });
    }
    (fixes, rejected)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeChat;

    fn source(segment: i64, text: &str) -> Source {
        Source {
            segment,
            text: text.into(),
        }
    }

    fn run<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn only_checked_changes_are_offered() {
        let reply = r#"好的：
```json
{"lines":[
  {"id":1,"text":"发到飞书群里"},
  {"id":2,"text":"[00:07] 对方：好的"},
  {"id":99,"text":"不存在的行"},
  {"id":"3","text":"这一行被改写成了一大段完全不同的、长得多的内容，远远超过原来的长度限制，应该被拒绝掉才对"},
  {"id":1,"text":"重复的修改"},
  {"id":4,"text":"第一行\n第二行"}
]}
```"#;
        let chat = FakeChat::new(100_000, move |_| Ok(reply.to_owned()));
        let lines = [
            source(11, "发到飞鼠群里"),
            source(12, "[00:07] 对方：好的"),
            source(13, "简短的一句"),
            source(14, "另一句"),
        ];
        let terms = vec!["飞书".to_owned()];
        let mut progress = Vec::new();
        let outcome = run(correct(
            &chat,
            &lines,
            &terms,
            Budget::default(),
            &Cancel::new(),
            &mut |d, t| progress.push((d, t)),
        ))
        .unwrap();
        assert_eq!(
            outcome.fixes,
            [Fix {
                segment: 11,
                text: "发到飞书群里".into()
            }]
        );
        assert_eq!(
            (outcome.rejected, outcome.failed_batches, outcome.batches),
            (4, 0, 1)
        );
        assert_eq!(progress, [(0, 1), (1, 1)]);
        let asked = &chat.asked()[0];
        assert!(
            asked.system.contains("飞书")
                && asked.user[0].contains(r#""id":1,"text":"发到飞鼠群里""#)
        );
        assert!(!asked.user[0].contains("11"), "segment ids stay in the app");
    }

    #[test]
    fn a_bad_batch_is_skipped_and_a_rejected_key_stops_everything() {
        let lines: Vec<Source> = (0..60)
            .map(|i| source(i, &format!("第{i}句话，我们讨论一下预算的安排和上线计划")))
            .collect();
        let budget = Budget {
            context_tokens: 1_200,
            max_output_tokens: 4_096,
        };
        let chat = FakeChat::new(100_000, |asked| {
            Ok(if asked.user[0].contains("第59句") {
                "not json".into()
            } else {
                r#"{"lines":[]}"#.into()
            })
        });
        let outcome = run(correct(
            &chat,
            &lines,
            &[],
            budget,
            &Cancel::new(),
            &mut |_, _| {},
        ))
        .unwrap();
        assert!(outcome.batches > 2, "{}", outcome.batches);
        assert_eq!((outcome.failed_batches, outcome.fixes.len()), (1, 0));
        let refused = FakeChat::new(100_000, |_| {
            Err(LlmError::Unauthorized {
                status: 401,
                detail: "bad key".into(),
            })
        });
        let result = run(correct(
            &refused,
            &lines,
            &[],
            budget,
            &Cancel::new(),
            &mut |_, _| {},
        ));
        assert!(matches!(result, Err(LlmError::Unauthorized { .. })));
    }
}
