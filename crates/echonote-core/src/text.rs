//! Cleanup of recognized text before it is stored. Ported from `asr-service`'s
//! `text_sanitizer.py`; lengths count characters, as in Python.
//!
//! The ASR decoder sometimes loops and repeats a phrase dozens of times. Units of at least 8
//! characters repeated at least 4 times in a row collapse to one, so ordinary repetition such
//! as "对对对" stays.

const MIN_REPEATED_UNIT_CHARS: usize = 8;
const MAX_REPEATED_UNIT_CHARS: usize = 120;
const MIN_REPETITIONS: usize = 4;
const MAX_SEGMENT_CHARS: usize = 3000;
const SENTENCE_ENDS: [char; 6] = ['。', '！', '？', '.', '!', '?'];

pub fn sanitize(text: &str) -> String {
    let text: Vec<char> = text.trim().chars().collect();
    if text.is_empty() {
        return String::new();
    }
    let collapsed = collapse_repeated_runs(&collapse_repeated_sentences(&text));
    let collapsed = trim_whitespace(&collapsed);
    if collapsed.len() <= MAX_SEGMENT_CHARS {
        return collapsed.iter().collect();
    }
    trim_at_boundary(collapsed, MAX_SEGMENT_CHARS)
}

/// Whole sentences repeated `MIN_REPETITIONS` times or more keep one copy, and a truncated
/// copy right after them is dropped.
fn collapse_repeated_sentences(text: &[char]) -> Vec<char> {
    let sentences = split_sentences(text);
    if sentences.len() < MIN_REPETITIONS {
        return text.to_vec();
    }
    let mut output = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < sentences.len() {
        let sentence = sentences[index];
        let mut repetitions = 1;
        while index + repetitions < sentences.len() && sentences[index + repetitions] == sentence {
            repetitions += 1;
        }
        index += repetitions;
        if repetitions >= MIN_REPETITIONS {
            output.extend_from_slice(sentence);
            if index < sentences.len() && sentence.starts_with(sentences[index]) {
                index += 1;
            }
            continue;
        }
        for _ in 0..repetitions {
            output.extend_from_slice(sentence);
        }
    }
    output
}

fn split_sentences(text: &[char]) -> Vec<&[char]> {
    let mut sentences = Vec::new();
    let mut start = 0;
    for (index, ch) in text.iter().enumerate() {
        if SENTENCE_ENDS.contains(ch) {
            sentences.push(&text[start..=index]);
            start = index + 1;
        }
    }
    if start < text.len() {
        sentences.push(&text[start..]);
    }
    sentences
}

fn collapse_repeated_runs(text: &[char]) -> Vec<char> {
    let mut output = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        match find_repeated_run(text, index) {
            Some((unit, repetitions)) => {
                output.extend_from_slice(&text[index..index + unit]);
                index += unit * repetitions;
            }
            None => {
                output.push(text[index]);
                index += 1;
            }
        }
    }
    output
}

/// The shortest unit starting at `index` that repeats at least `MIN_REPETITIONS` times.
fn find_repeated_run(text: &[char], index: usize) -> Option<(usize, usize)> {
    let remaining = text.len() - index;
    let longest = MAX_REPEATED_UNIT_CHARS.min(remaining / MIN_REPETITIONS);
    (MIN_REPEATED_UNIT_CHARS..=longest).find_map(|length| {
        let unit = &text[index..index + length];
        if unit.iter().all(|ch| ch.is_whitespace()) {
            return None;
        }
        let mut repetitions = 1;
        let mut cursor = index + length;
        while text[cursor..].starts_with(unit) {
            repetitions += 1;
            cursor += length;
        }
        (repetitions >= MIN_REPETITIONS).then_some((length, repetitions))
    })
}

fn trim_whitespace(text: &[char]) -> &[char] {
    let start = text
        .iter()
        .position(|ch| !ch.is_whitespace())
        .unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|ch| !ch.is_whitespace())
        .map_or(start, |end| end + 1);
    &text[start..end]
}

/// Cut to `max_chars`, at the last sentence end in its second half if there is one. Ends are
/// tried in `SENTENCE_ENDS` order, as in the Python original.
fn trim_at_boundary(text: &[char], max_chars: usize) -> String {
    let candidate = &text[..max_chars];
    let candidate = &candidate[..candidate
        .iter()
        .rposition(|ch| !ch.is_whitespace())
        .map_or(0, |end| end + 1)];
    for boundary in SENTENCE_ENDS {
        if let Some(position) = candidate.iter().rposition(|ch| *ch == boundary) {
            if position >= max_chars / 2 {
                return candidate[..=position].iter().collect();
            }
        }
    }
    candidate.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_runaway_repetition() {
        let repeated = "因为这块的，呃，客户通过四点零的资源也可以盘活。";
        let text = format!(
            "主要是他可以，就有一个优点，就是四点零的资源也可以盘活。{}因为这块的，呃，客户通过",
            repeated.repeat(20)
        );
        assert_eq!(
            sanitize(&text),
            format!("主要是他可以，就有一个优点，就是四点零的资源也可以盘活。{repeated}")
        );
    }

    #[test]
    fn preserves_normal_repetition() {
        let text = "对对对，这个可以。嗯嗯，我们先按五十台测算，然后再看后续需求。";
        assert_eq!(sanitize(text), text);
        assert_eq!(sanitize("好的。好的。好的。"), "好的。好的。好的。");
    }

    #[test]
    fn collapses_a_loop_without_sentence_ends() {
        let text = format!("let us look at{}", " the next slide".repeat(6));
        assert_eq!(sanitize(&text), "let us look at the next slide");
    }

    #[test]
    fn caps_unstructured_runaway_output() {
        let text = "没有明显循环边界".repeat(500);
        assert!(sanitize(&text).chars().count() <= MAX_SEGMENT_CHARS);
        // Long output without repeats or sentence ends is cut to the cap.
        let long = (0..1000)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let cut = sanitize(&long);
        assert!(long.starts_with(&cut));
        assert!((MAX_SEGMENT_CHARS - 5..=MAX_SEGMENT_CHARS).contains(&cut.chars().count()));
    }

    #[test]
    fn empty_and_blank_text_is_empty() {
        assert_eq!(sanitize(""), "");
        assert_eq!(sanitize("  \n "), "");
    }
}
