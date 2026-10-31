//! Running highlights while a meeting is recorded: the current bullets plus the transcript
//! since the last update go in, at most [`MAX_BULLETS`] bullets come back. Runs beside the
//! recording and never touches it; a failed update keeps the previous bullets.

use crate::client::{Chat, Message, Request};
use crate::summary::Budget;
use crate::transcript::{Language, Line, sanitize_markdown};
use crate::{Cancel, LlmError, complete, prompts, tokens};

pub const MAX_BULLETS: usize = 8;

pub struct Update<'a> {
    /// The bullets so far, as returned last time; empty at first.
    pub current: &'a str,
    /// Lines since the last update, oldest first.
    pub new_lines: &'a [Line],
    pub terms: &'a [String],
    pub budget: Budget,
}

pub async fn update<C: Chat>(
    chat: &C,
    update: &Update<'_>,
    cancel: &Cancel,
) -> Result<String, LlmError> {
    let language = Language::of(update.new_lines);
    let system = prompts::highlights(language, update.terms);
    let room = update
        .budget
        .input()
        .saturating_sub(tokens::estimate(&system) + tokens::estimate(update.current) + 64);
    // The newest lines when there are more than fit; older ones are in the bullets already.
    let mut used = 0;
    let mut start = update.new_lines.len();
    while start > 0 {
        let cost = tokens::estimate(&update.new_lines[start - 1].text) + 1;
        if used + cost > room {
            break;
        }
        used += cost;
        start -= 1;
    }
    let lines: Vec<&str> = update.new_lines[start..]
        .iter()
        .map(|line| line.text.as_str())
        .collect();
    if lines.is_empty() {
        return Ok(update.current.to_owned());
    }
    let current = if update.current.trim().is_empty() {
        "-".to_owned()
    } else {
        update.current.trim().to_owned()
    };
    let content = format!(
        "<highlights>\n{current}\n</highlights>\n\n{}",
        prompts::wrap(&lines.join("\n"))
    );
    let request = Request {
        system,
        messages: vec![Message::user(content)],
        max_tokens: update.budget.output().min(1_024) as u32,
        temperature: Some(0.2),
    };
    let (text, _) = complete(chat, &request, cancel).await?;
    let bullets = bullets(&text);
    if bullets.is_empty() {
        return Err(LlmError::BadResponse("the highlights have no list items"));
    }
    Ok(bullets)
}

/// The list items of `text`, at most [`MAX_BULLETS`], as `- item` lines.
fn bullets(text: &str) -> String {
    let items: Vec<String> = text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let item = line
                .strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
                .or_else(|| line.strip_prefix("• "))
                .or_else(|| {
                    let (number, rest) = line.split_once(". ")?;
                    number.chars().all(|ch| ch.is_ascii_digit()).then_some(rest)
                })?;
            let item = item.trim();
            (!item.is_empty()).then(|| format!("- {item}"))
        })
        .take(MAX_BULLETS)
        .collect();
    sanitize_markdown(&items.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeChat;

    #[test]
    fn only_list_items_are_kept() {
        let text = "好的，更新如下：\n- 确定预算 [01:02]\n* 下周上线 [03:00]\n1. 张三负责测试 [04:10]\n\n结束";
        assert_eq!(
            bullets(text),
            "- 确定预算 [01:02]\n- 下周上线 [03:00]\n- 张三负责测试 [04:10]"
        );
        let many: String = (0..12).map(|i| format!("- 第{i}条\n")).collect();
        assert_eq!(bullets(&many).lines().count(), MAX_BULLETS);
    }

    #[test]
    fn an_update_sends_the_bullets_and_the_new_lines() {
        let chat = FakeChat::new(100_000, |_| Ok("- 确定预算 [00:05]".into()));
        let lines = [Line {
            segment: 1,
            start_ms: 5_000,
            text: "[00:05] 我：预算定了".into(),
        }];
        let update = Update {
            current: "- 讨论了上线时间 [00:01]",
            new_lines: &lines,
            terms: &[],
            budget: Budget::default(),
        };
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(super::update(&chat, &update, &Cancel::new()))
            .unwrap();
        assert_eq!(result, "- 确定预算 [00:05]");
        let asked = &chat.asked()[0];
        assert!(asked.user[0].starts_with("<highlights>\n- 讨论了上线时间"));
        assert!(asked.user[0].contains("<transcript>\n[00:05] 我：预算定了"));
    }
}
