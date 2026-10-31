//! Questions about one meeting. The whole transcript goes in when it fits; a longer one is
//! cut down to the lines that share words with the question (BM25 over words and CJK
//! characters and character pairs), with their neighbours, in time order. The latest notes
//! go in too when there are any: they cover questions about the meeting as a whole.

use std::collections::{BTreeSet, HashMap};

use crate::client::{Chat, Message, Request};
use crate::summary::Budget;
use crate::transcript::{Language, Line, join, sanitize_markdown, split_text};
use crate::{Cancel, LlmError, prompts, tokens};

/// Earlier questions and answers kept for context.
const MAX_TURNS: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub question: String,
    pub answer: String,
}

pub struct Ask<'a> {
    pub lines: &'a [Line],
    pub question: &'a str,
    /// Oldest first.
    pub history: &'a [Turn],
    /// The meeting's latest notes (summary), if any.
    pub notes: Option<&'a str>,
    pub terms: &'a [String],
    pub budget: Budget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// Markdown with `[mm:ss]` citations; images and HTML made text.
    pub text: String,
    pub truncated: bool,
    /// Only parts of the transcript were sent.
    pub excerpt: bool,
}

pub async fn answer<C: Chat>(
    chat: &C,
    ask: &Ask<'_>,
    cancel: &Cancel,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<Answer, LlmError> {
    let language = Language::of(ask.lines);
    let input = ask.budget.input();
    let question = ask.question.trim();
    let mut history: Vec<&Turn> = Vec::new();
    let mut used = 0;
    for turn in ask.history.iter().rev().take(MAX_TURNS) {
        let cost = tokens::estimate(&turn.question) + tokens::estimate(&turn.answer) + 8;
        if used + cost > input * 15 / 100 {
            break;
        }
        used += cost;
        history.push(turn);
    }
    history.reverse();
    let notes = ask
        .notes
        .filter(|notes| !notes.trim().is_empty())
        .map(|notes| split_text(notes, input / 5).swap_remove(0));
    let fixed = tokens::estimate(&prompts::answer(language, ask.terms, true, notes.is_some()))
        + used
        + tokens::estimate(question)
        + notes.as_deref().map_or(0, tokens::estimate)
        + 32;
    let (transcript, excerpt) = select(ask.lines, question, input.saturating_sub(fixed));
    let mut system = prompts::answer(language, ask.terms, excerpt, notes.is_some());
    if let Some(notes) = &notes {
        system.push_str(&format!("\n\n<notes>\n{notes}\n</notes>"));
    }
    system.push_str("\n\n");
    system.push_str(&prompts::wrap(&transcript));
    let mut messages = Vec::new();
    for turn in history {
        messages.push(Message::user(turn.question.clone()));
        messages.push(Message::assistant(turn.answer.clone()));
    }
    messages.push(Message::user(question));
    let request = Request {
        system,
        messages,
        max_tokens: ask.budget.output().min(2_048) as u32,
        temperature: Some(0.2),
    };
    let mut text = String::new();
    let finish = chat
        .stream(&request, cancel, &mut |chunk: &str| {
            text.push_str(chunk);
            on_text(chunk);
        })
        .await?;
    Ok(Answer {
        text: sanitize_markdown(text.trim()),
        truncated: finish.truncated,
        excerpt,
    })
}

fn is_ideograph(ch: char) -> bool {
    matches!(ch as u32,
        0x3040..=0x30ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff
        | 0x20000..=0x2ffff)
}

/// The words of a line: lowercased Latin words and numbers of two or more characters, CJK
/// character pairs, and single CJK characters when `unigrams` (or when a run is one long).
fn terms(text: &str, unigrams: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut run: Vec<char> = Vec::new();
    let end_word = |word: &mut String, out: &mut Vec<String>| {
        if word.chars().count() >= 2 {
            out.push(word.clone());
        }
        word.clear();
    };
    let end_run = |run: &mut Vec<char>, out: &mut Vec<String>| {
        out.extend(run.windows(2).map(|pair| pair.iter().collect::<String>()));
        if unigrams || run.len() == 1 {
            out.extend(run.iter().map(char::to_string));
        }
        run.clear();
    };
    for ch in text.chars() {
        if is_ideograph(ch) {
            end_word(&mut word, &mut out);
            run.push(ch);
        } else if ch.is_alphanumeric() {
            end_run(&mut run, &mut out);
            word.extend(ch.to_lowercase());
        } else {
            end_word(&mut word, &mut out);
            end_run(&mut run, &mut out);
        }
    }
    end_word(&mut word, &mut out);
    end_run(&mut run, &mut out);
    out
}

/// A line without its `[mm:ss] 我：` prefix.
fn content(text: &str) -> &str {
    let text = text
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("] ").map(|(_, rest)| rest))
        .unwrap_or(text);
    for separator in ["：", ": "] {
        if let Some((label, rest)) = text.split_once(separator)
            && label.chars().count() <= 4
        {
            return rest;
        }
    }
    text
}

/// BM25 score of each line for `question`.
fn scores(lines: &[Line], question: &str) -> Vec<f64> {
    let query: BTreeSet<String> = terms(question, false).into_iter().collect();
    let docs: Vec<Vec<String>> = lines
        .iter()
        .map(|line| terms(content(&line.text), true))
        .collect();
    let total = docs.len().max(1) as f64;
    let average = docs.iter().map(Vec::len).sum::<usize>() as f64 / total;
    let mut frequency: HashMap<&str, usize> = HashMap::new();
    for doc in &docs {
        let unique: BTreeSet<&str> = doc.iter().map(String::as_str).collect();
        for term in unique {
            *frequency.entry(term).or_default() += 1;
        }
    }
    let (k1, b) = (1.2, 0.75);
    docs.iter()
        .map(|doc| {
            query
                .iter()
                .map(|term| {
                    let tf = doc.iter().filter(|t| *t == term).count() as f64;
                    if tf == 0.0 {
                        return 0.0;
                    }
                    let df = frequency.get(term.as_str()).copied().unwrap_or(0) as f64;
                    let idf = (1.0 + (total - df + 0.5) / (df + 0.5)).ln();
                    let norm = k1 * (1.0 - b + b * doc.len() as f64 / average.max(1.0));
                    idf * tf * (k1 + 1.0) / (tf + norm)
                })
                .sum()
        })
        .collect()
}

/// The transcript, or the lines most related to `question` with their neighbours, within
/// `room` tokens. Returns the text and whether it is an excerpt.
fn select(lines: &[Line], question: &str, room: usize) -> (String, bool) {
    let all = join(lines);
    if tokens::estimate(&all) <= room {
        return (all, false);
    }
    let scores = scores(lines, question);
    let mut order: Vec<usize> = (0..lines.len()).filter(|&i| scores[i] > 0.0).collect();
    order.sort_by(|a, b| scores[*b].total_cmp(&scores[*a]).then(a.cmp(b)));
    if order.is_empty() {
        // Nothing in common: the start and the end, where agendas and conclusions are.
        let (mut front, mut back) = (0, lines.len());
        while front < back {
            order.push(front);
            front += 1;
            if front < back {
                back -= 1;
                order.push(back);
            }
        }
    }
    let mut chosen = BTreeSet::new();
    let mut used = 0;
    'fill: for index in order {
        for neighbour in [index, index.wrapping_sub(1), index + 1] {
            if neighbour >= lines.len() || chosen.contains(&neighbour) {
                continue;
            }
            let cost = tokens::estimate(&lines[neighbour].text) + 1;
            if used + cost > room {
                break 'fill;
            }
            used += cost;
            chosen.insert(neighbour);
        }
    }
    let mut out = Vec::new();
    let mut previous: Option<usize> = None;
    for index in chosen {
        if previous.is_some_and(|p| p + 1 != index) || (previous.is_none() && index > 0) {
            out.push("…");
        }
        out.push(lines[index].text.as_str());
        previous = Some(index);
    }
    if previous.is_some_and(|p| p + 1 < lines.len()) {
        out.push("…");
    }
    (out.join("\n"), true)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeChat;

    fn line(index: usize, text: &str) -> Line {
        Line {
            segment: index as i64 + 1,
            start_ms: index as i64 * 5_000,
            text: format!("[{:02}:{:02}] 我：{text}", index * 5 / 60, index * 5 % 60),
        }
    }

    fn meeting(count: usize, target: usize) -> Vec<Line> {
        (0..count)
            .map(|i| {
                if i == target {
                    line(i, "明年的预算是三百万")
                } else {
                    line(i, "今天天气不错我们聊聊别的事情")
                }
            })
            .collect()
    }

    #[test]
    fn terms_are_words_and_character_pairs() {
        assert_eq!(
            terms("我们讨论 Kubernetes 预算 v2，好", false),
            ["我们", "们讨", "讨论", "kubernetes", "预算", "v2", "好"]
        );
        assert!(terms("预算", true).contains(&"预".to_owned()));
        assert_eq!(content("[01:02:03] 对方：内容: 细节"), "内容: 细节");
        assert_eq!(content("[00:05] no label here"), "no label here");
    }

    #[test]
    fn a_long_transcript_is_cut_to_the_relevant_lines() {
        let lines = meeting(2_000, 1_200);
        let (text, excerpt) = select(&lines, "明年预算是多少？", 300);
        assert!(excerpt);
        assert!(text.contains("明年的预算是三百万"), "{text}");
        // Its neighbours come along, in time order, with gaps marked.
        let shown: Vec<&str> = text.lines().collect();
        let target = shown.iter().position(|l| l.contains("三百万")).unwrap();
        assert_eq!(shown[target - 1], lines[1_199].text);
        assert_eq!(shown[target + 1], lines[1_201].text);
        assert!(shown.contains(&"…"));
        assert!(tokens::estimate(&text) <= 300 + 20);
        let (whole, excerpt) = select(&lines[..10], "预算", 10_000);
        assert!(!excerpt && whole == join(&lines[..10]));
    }

    #[test]
    fn answers_carry_the_transcript_notes_and_recent_turns() {
        let chat = FakeChat::new(100_000, |_| Ok("预算是三百万 [01:40]。".into()));
        let lines = meeting(30, 20);
        let history = vec![Turn {
            question: "谁主持的？".into(),
            answer: "没有提到。".into(),
        }];
        let ask = Ask {
            lines: &lines,
            question: " 明年预算是多少？ ",
            history: &history,
            notes: Some("## 概要\n讨论了预算。"),
            terms: &[],
            budget: Budget::default(),
        };
        let mut streamed = String::new();
        let answer = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(super::answer(&chat, &ask, &Cancel::new(), &mut |c| {
                streamed.push_str(c)
            }))
            .unwrap();
        assert_eq!(answer.text, "预算是三百万 [01:40]。");
        assert!(!answer.excerpt && streamed == answer.text);
        let asked = &chat.asked()[0];
        assert!(
            asked.system.contains("<notes>\n## 概要")
                && asked.system.contains("明年的预算是三百万")
        );
        assert!(asked.system.contains("[分:秒]"));
        assert_eq!(asked.user, ["谁主持的？", "没有提到。", "明年预算是多少？"]);
    }
}
