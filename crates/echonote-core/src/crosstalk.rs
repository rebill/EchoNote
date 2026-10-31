//! Speaker bleed suppression. Without headphones the microphone picks up the other side from
//! the speakers about 10 dB down, and the ASR transcribes it almost verbatim (M0). A mic final
//! is compared with system-audio tokens of the same moment: tokens are matched in order, only
//! when their start times agree within the acoustic lag, and only runs of matches count, so
//! a common word said at the same time is not taken for bleed.
//!
//! Times are seconds on the session clock, so callers convert each track's timeline first.
//! Models without token timestamps are not supported.

use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct CrosstalkConfig {
    /// Allowed `mic - sys` start-time difference of a matched token. Bleed arrives 0–200 ms
    /// late; both sides carry about one 60 ms model frame of timestamp jitter.
    pub min_lag_s: f64,
    pub max_lag_s: f64,
    /// Matched tokens count as bleed only in runs at least this long, or when they are the
    /// whole final and at least `min_whole` long (short echoes such as a time of day).
    pub min_run: usize,
    pub min_whole: usize,
    /// After removing bleed the rest is kept only with this many words and this share.
    pub min_kept: usize,
    pub min_kept_share: f64,
}

impl Default for CrosstalkConfig {
    fn default() -> Self {
        Self {
            min_lag_s: -0.3,
            max_lag_s: 0.5,
            min_run: 3,
            min_whole: 2,
            min_kept: 4,
            min_kept_share: 0.3,
        }
    }
}

/// A recognized token on the session clock.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub at_s: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// Not bleed.
    Keep,
    /// Partly bleed: `text` is what the user said.
    Trim { text: String },
    /// Bleed: hide it, keeping it as a duplicate of the system track.
    Duplicate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Judgement {
    pub verdict: Verdict,
    /// Share of the mic final's words matched in time (diagnostic).
    pub similarity: f64,
    pub words: usize,
    pub removed: usize,
}

#[derive(Clone, Debug)]
struct RefWord {
    key: String,
    at_s: f64,
}

/// Recent system-audio tokens: finals, plus the preview of the utterance still being spoken
/// so a mic final can be judged before the system final arrives. A final can arrive after the
/// next utterance has started, so it leaves the preview alone; clear the preview when the
/// system utterance changes. Words present in both only add alternative matches.
#[derive(Default)]
pub struct Reference {
    finals: VecDeque<RefWord>,
    preview: Vec<RefWord>,
}

impl Reference {
    pub fn add_final(&mut self, words: &[Word]) {
        self.finals.extend(ref_words(words));
    }

    pub fn set_preview(&mut self, words: &[Word]) {
        self.preview = ref_words(words).collect();
    }

    pub fn clear_preview(&mut self) {
        self.preview.clear();
    }

    /// Forget finals that no mic final can reach any more.
    pub fn prune_before(&mut self, at_s: f64) {
        while self.finals.front().is_some_and(|word| word.at_s < at_s) {
            self.finals.pop_front();
        }
    }

    pub fn judge(&self, mic: &[Word], config: &CrosstalkConfig) -> Judgement {
        let keyed: Vec<(usize, String, f64)> = mic
            .iter()
            .enumerate()
            .filter_map(|(index, word)| key(&word.text).map(|key| (index, key, word.at_s)))
            .collect();
        let keep_all = |similarity| Judgement {
            verdict: Verdict::Keep,
            similarity,
            words: keyed.len(),
            removed: 0,
        };
        let (Some(first), Some(last)) = (keyed.first(), keyed.last()) else {
            return keep_all(0.0);
        };
        let (from, to) = (first.2 - config.max_lag_s, last.2 - config.min_lag_s);
        let mut reference: Vec<&RefWord> = self
            .finals
            .iter()
            .chain(&self.preview)
            .filter(|word| word.at_s >= from && word.at_s <= to)
            .collect();
        reference.sort_by(|a, b| a.at_s.total_cmp(&b.at_s));
        let matched = align(&keyed, &reference, config);
        let total_matched = matched.iter().filter(|value| **value).count();
        let similarity = total_matched as f64 / keyed.len() as f64;

        // Bleed words: matched runs of at least `min_run`, or a fully matched short final.
        let mut bleed = vec![false; keyed.len()];
        if total_matched == keyed.len() && keyed.len() >= config.min_whole {
            bleed.fill(true);
        }
        let mut run_start = 0;
        for index in 0..=keyed.len() {
            if index < keyed.len() && matched[index] {
                continue;
            }
            if index - run_start >= config.min_run {
                bleed[run_start..index].fill(true);
            }
            run_start = index + 1;
        }
        let removed = bleed.iter().filter(|value| **value).count();
        if removed == 0 {
            return keep_all(similarity);
        }
        let kept = keyed.len() - removed;
        let verdict = if kept < config.min_kept
            || (kept as f64) < config.min_kept_share * keyed.len() as f64
        {
            Verdict::Duplicate
        } else {
            Verdict::Trim {
                text: rebuild(mic, &keyed, &bleed),
            }
        };
        Judgement {
            verdict,
            similarity,
            words: keyed.len(),
            removed,
        }
    }
}

fn ref_words(words: &[Word]) -> impl Iterator<Item = RefWord> + '_ {
    words.iter().filter_map(|word| {
        key(&word.text).map(|key| RefWord {
            key,
            at_s: word.at_s,
        })
    })
}

/// Comparable form of a token, or `None` for punctuation and model tags.
fn key(text: &str) -> Option<String> {
    if text.starts_with("<|") {
        return None;
    }
    let key: String = text
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    (!key.is_empty()).then_some(key)
}

/// Longest common subsequence where two tokens match only if their texts are equal and their
/// start times agree within the lag window. Returns, per mic word, whether it was matched.
fn align(
    mic: &[(usize, String, f64)],
    reference: &[&RefWord],
    config: &CrosstalkConfig,
) -> Vec<bool> {
    let matches = |i: usize, j: usize| {
        let lag = mic[i].2 - reference[j].at_s;
        mic[i].1 == reference[j].key && lag >= config.min_lag_s && lag <= config.max_lag_s
    };
    let (rows, columns) = (mic.len(), reference.len());
    let mut table = vec![0u32; (rows + 1) * (columns + 1)];
    let at = |i: usize, j: usize| i * (columns + 1) + j;
    for i in 1..=rows {
        for j in 1..=columns {
            table[at(i, j)] = if matches(i - 1, j - 1) {
                table[at(i - 1, j - 1)] + 1
            } else {
                table[at(i - 1, j)].max(table[at(i, j - 1)])
            };
        }
    }
    let mut matched = vec![false; rows];
    let (mut i, mut j) = (rows, columns);
    while i > 0 && j > 0 {
        if matches(i - 1, j - 1) && table[at(i, j)] == table[at(i - 1, j - 1)] + 1 {
            matched[i - 1] = true;
            i -= 1;
            j -= 1;
        } else if table[at(i - 1, j)] >= table[at(i, j - 1)] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    matched
}

/// Text of the mic tokens that are not bleed. Punctuation after a removed word goes with it.
fn rebuild(mic: &[Word], keyed: &[(usize, String, f64)], bleed: &[bool]) -> String {
    let mut removed = vec![false; mic.len()];
    for ((index, _, _), is_bleed) in keyed.iter().zip(bleed) {
        removed[*index] = *is_bleed;
    }
    let mut text = String::new();
    let mut previous_removed = false;
    for (word, removed) in mic.iter().zip(removed) {
        let is_word = key(&word.text).is_some();
        if is_word {
            previous_removed = removed;
        }
        if removed || (!is_word && previous_removed) {
            continue;
        }
        text.push_str(&word.text.replace('\u{2581}', " "));
    }
    text.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One word per character, `step_s` apart from `start_s`.
    fn words(text: &str, start_s: f64, step_s: f64) -> Vec<Word> {
        text.chars()
            .enumerate()
            .map(|(index, ch)| Word {
                text: ch.to_string(),
                at_s: start_s + index as f64 * step_s,
            })
            .collect()
    }

    fn judge(sys: &[Word], mic: &[Word]) -> Judgement {
        let mut reference = Reference::default();
        reference.add_final(sys);
        reference.judge(mic, &CrosstalkConfig::default())
    }

    #[test]
    fn bleed_with_recognition_errors_is_a_duplicate() {
        // Real pair from M0: the mic heard 不为阴谋 where the speakers said 互为因果.
        let sys = words(
            "其实我觉得就是第一个啊睡眠和情绪它确实是互为因果",
            12.20,
            0.15,
        );
        let mic = words(
            "其实我觉得就是第一个啊睡眠和情绪它确实是不为阴谋",
            12.25,
            0.15,
        );
        let judgement = judge(&sys, &mic);
        assert_eq!(judgement.verdict, Verdict::Duplicate);
        assert!(judgement.similarity > 0.8);
    }

    #[test]
    fn the_same_words_at_another_time_are_not_bleed() {
        let sys = words("我们下周再讨论这个方案", 10.0, 0.2);
        let mic = words("我们下周再讨论这个方案", 14.0, 0.2);
        assert_eq!(judge(&sys, &mic).verdict, Verdict::Keep);
    }

    #[test]
    fn isolated_common_words_during_double_talk_are_kept() {
        let sys = words("我们的预算还有多少", 10.0, 0.2);
        let mic = words("我觉得的确需要", 10.0, 0.2);
        let judgement = judge(&sys, &mic);
        assert_eq!(judgement.verdict, Verdict::Keep);
        assert_eq!(judgement.removed, 0);
    }

    #[test]
    fn own_words_after_bleed_are_kept_and_bleed_is_trimmed() {
        let sys = words("明天上午十点开会", 10.0, 0.2);
        let mut mic = words("明天上午十点开会", 10.05, 0.2);
        mic.push(Word {
            text: "，".into(),
            at_s: 11.65,
        });
        mic.extend(words("好的我会准时参加", 12.5, 0.2));
        let judgement = judge(&sys, &mic);
        assert_eq!(
            judgement.verdict,
            Verdict::Trim {
                text: "好的我会准时参加".into()
            }
        );
        assert_eq!(judgement.removed, 8);
    }

    #[test]
    fn a_short_final_is_a_duplicate_only_when_it_matches_entirely() {
        let sys = words("八点苹果在人才战争中", 5.0, 0.2);
        assert_eq!(
            judge(&sys, &words("八点", 5.1, 0.2)).verdict,
            Verdict::Duplicate
        );
        assert_eq!(judge(&sys, &words("八", 5.1, 0.2)).verdict, Verdict::Keep);
        assert_eq!(judge(&sys, &words("八天", 5.1, 0.2)).verdict, Verdict::Keep);
    }

    #[test]
    fn short_bleed_with_little_left_is_a_duplicate() {
        let sys = words("对对对没问题", 20.0, 0.2);
        let mut mic = words("对对对没问题", 20.1, 0.2);
        mic.extend(words("嗯", 21.5, 0.2));
        assert_eq!(judge(&sys, &mic).verdict, Verdict::Duplicate);
    }

    #[test]
    fn previews_count_until_cleared() {
        let mut reference = Reference::default();
        reference.set_preview(&words("今天的议题有三个", 5.0, 0.2));
        let mic = words("今天的议题有三个", 5.1, 0.2);
        let config = CrosstalkConfig::default();
        assert_eq!(reference.judge(&mic, &config).verdict, Verdict::Duplicate);
        reference.clear_preview();
        reference.add_final(&words("今天", 5.0, 0.2));
        assert_eq!(reference.judge(&mic, &config).verdict, Verdict::Keep);
        reference.prune_before(100.0);
        assert_eq!(reference.judge(&mic, &config).similarity, 0.0);
    }

    #[test]
    fn english_word_pieces_and_tags_are_normalized() {
        let sys = vec![
            Word {
                text: "<|en|>".into(),
                at_s: 1.0,
            },
            Word {
                text: "\u{2581}Hello".into(),
                at_s: 1.0,
            },
            Word {
                text: "\u{2581}team".into(),
                at_s: 1.3,
            },
            Word {
                text: "\u{2581}stand".into(),
                at_s: 1.6,
            },
            Word {
                text: "up".into(),
                at_s: 1.8,
            },
        ];
        let mic: Vec<Word> = sys
            .iter()
            .skip(1)
            .map(|word| Word {
                text: word.text.to_lowercase(),
                at_s: word.at_s + 0.1,
            })
            .collect();
        let judgement = judge(&sys, &mic);
        assert_eq!(judgement.words, 4);
        assert_eq!(judgement.verdict, Verdict::Duplicate);
    }
}
