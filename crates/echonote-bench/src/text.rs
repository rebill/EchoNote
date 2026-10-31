//! Text normalization for error-rate scoring.
//!
//! CJK characters count as one token each and Latin words count as one token, so the same
//! function scores Mandarin (CER), English (WER) and code-switched text (MER).

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result};

static T2S: OnceLock<HashMap<char, char>> = OnceLock::new();

/// Fold Traditional characters to Simplified before scoring, using a JSON {"開": "开"} map
/// (generated from OpenCC TSCharacters by the M0 tooling).
pub fn enable_t2s(path: &Path) -> Result<()> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let pairs: HashMap<String, String> = serde_json::from_str(&raw)?;
    let map = pairs
        .into_iter()
        .filter_map(|(from, to)| Some((from.chars().next()?, to.chars().next()?)))
        .collect();
    let _ = T2S.set(map);
    Ok(())
}

/// Tokenize text for scoring: strip `<|tag|>` markers and punctuation, fold width and case.
/// With `cn_numerals`, Arabic digit runs are rewritten as Chinese numerals so references that
/// spell numbers out (AISHELL) are not penalized against models that apply ITN.
pub fn tokens(text: &str, cn_numerals: bool) -> Vec<String> {
    let stripped = strip_tags(text);
    let t2s = T2S.get();
    let folded: String = stripped
        .chars()
        .map(fold_char)
        .map(|ch| t2s.and_then(|map| map.get(&ch).copied()).unwrap_or(ch))
        .collect();
    let folded = if cn_numerals {
        digits_to_chinese(&folded)
    } else {
        folded
    };

    let mut tokens = Vec::new();
    let mut word = String::new();
    for ch in folded.chars() {
        if is_cjk(ch) {
            flush(&mut word, &mut tokens);
            tokens.push(ch.to_string());
        } else if ch.is_alphanumeric() || (ch == '\'' && !word.is_empty()) {
            word.push(ch);
        } else {
            flush(&mut word, &mut tokens);
        }
    }
    flush(&mut word, &mut tokens);
    tokens
}

fn flush(word: &mut String, tokens: &mut Vec<String>) {
    let trimmed = word.trim_end_matches('\'');
    if !trimmed.is_empty() {
        tokens.push(trimmed.to_string());
    }
    word.clear();
}

fn strip_tags(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<|") {
        output.push_str(&rest[..start]);
        match rest[start..].find("|>") {
            Some(end) => rest = &rest[start + end + 2..],
            None => {
                rest = "";
            }
        }
    }
    output.push_str(rest);
    output
}

fn fold_char(ch: char) -> char {
    let ch = match ch {
        '\u{3000}' => ' ',
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(ch as u32 - 0xFEE0).unwrap_or(ch),
        '\u{2018}' | '\u{2019}' => '\'',
        _ => ch,
    };
    ch.to_lowercase().next().unwrap_or(ch)
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x3040..=0x30FF   // Hiragana, Katakana
        | 0x3400..=0x4DBF // CJK Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xAC00..=0xD7AF // Hangul syllables
        | 0xF900..=0xFAFF // CJK Compatibility Ideographs
    ) || ch == '〇'
}

const DIGITS: [char; 10] = ['零', '一', '二', '三', '四', '五', '六', '七', '八', '九'];

/// Rewrite runs of ASCII digits as Chinese numerals. Runs with a leading zero, more than eight
/// digits, or a decimal part are read digit by digit; others are read as cardinal numbers.
fn digits_to_chinese(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() {
            output.push(chars[index]);
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len() && chars[index].is_ascii_digit() {
            index += 1;
        }
        let run: String = chars[start..index].iter().collect();
        let followed_by_point =
            index + 1 < chars.len() && chars[index] == '.' && chars[index + 1].is_ascii_digit();
        let percent = index < chars.len() && chars[index] == '%';
        let year = run.len() == 4 && chars.get(index) == Some(&'年');
        let spoken = if year || run.len() > 1 && run.starts_with('0') || run.len() > 8 {
            read_digits(&run)
        } else {
            read_cardinal(run.parse().unwrap_or(0))
        };
        if percent {
            output.push_str("百分之");
            output.push_str(&spoken);
            index += 1;
        } else {
            output.push_str(&spoken);
        }
        if followed_by_point {
            output.push('点');
            index += 1;
            let fraction_start = index;
            while index < chars.len() && chars[index].is_ascii_digit() {
                index += 1;
            }
            let fraction: String = chars[fraction_start..index].iter().collect();
            output.push_str(&read_digits(&fraction));
        }
    }
    output
}

fn read_digits(run: &str) -> String {
    run.chars()
        .map(|digit| DIGITS[digit.to_digit(10).unwrap_or(0) as usize])
        .collect()
}

fn read_cardinal(value: u64) -> String {
    if value == 0 {
        return "零".into();
    }
    let high = value / 10_000;
    let low = value % 10_000;
    let mut output = String::new();
    if high > 0 {
        output.push_str(&read_below_10k(high));
        output.push('万');
        if low > 0 && low < 1_000 {
            output.push('零');
        }
    }
    if low > 0 {
        output.push_str(&read_below_10k(low));
    }
    // A leading 一十 is spoken 十 (十二, 十万).
    match output.strip_prefix("一十") {
        Some(rest) => format!("十{rest}"),
        None => output,
    }
}

fn read_below_10k(value: u64) -> String {
    const UNITS: [&str; 4] = ["千", "百", "十", ""];
    let digits = [
        value / 1000 % 10,
        value / 100 % 10,
        value / 10 % 10,
        value % 10,
    ];
    let mut output = String::new();
    let mut pending_zero = false;
    let mut started = false;
    for (digit, unit) in digits.iter().zip(UNITS) {
        if *digit == 0 {
            pending_zero = started;
            continue;
        }
        if pending_zero {
            output.push('零');
            pending_zero = false;
        }
        output.push(DIGITS[*digit as usize]);
        output.push_str(unit);
        started = true;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(text: &str, cn: bool) -> String {
        tokens(text, cn).join(" ")
    }

    #[test]
    fn splits_cjk_characters_and_latin_words() {
        assert_eq!(
            joined("我们用 GPT-4 做 demo。", false),
            "我 们 用 gpt 4 做 demo"
        );
        assert_eq!(joined("<|zh|><|NEUTRAL|><|Speech|>你好", false), "你 好");
        assert_eq!(joined("DON'T stop", false), "don't stop");
        assert_eq!(joined("ＡＢＣ１２３", false), "abc123");
    }

    #[test]
    fn converts_digits_to_spoken_chinese() {
        assert_eq!(digits_to_chinese("12"), "十二");
        assert_eq!(digits_to_chinese("2015年"), "二零一五年");
        assert_eq!(digits_to_chinese("2015"), "二千零一十五");
        assert_eq!(digits_to_chinese("100010"), "十万零一十");
        assert_eq!(digits_to_chinese("10005"), "一万零五");
        assert_eq!(digits_to_chinese("110"), "一百一十");
        assert_eq!(digits_to_chinese("3.14"), "三点一四");
        assert_eq!(digits_to_chinese("007"), "零零七");
        assert_eq!(digits_to_chinese("50%"), "百分之五十");
        assert_eq!(digits_to_chinese("1000000"), "一百万");
    }
}
