//! The user's glossary: terms with the ways the recognizer gets them wrong. Rule replacement
//! happens when text is shown or exported; stored text stays what was recognized or typed, so
//! changing the glossary changes every meeting at once. The same terms are hotwords for the
//! high-accuracy model and context for LLM prompts.
//!
//! Matching scans left to right and takes the longest match at each position, so a
//! replacement is never matched again. A term matches itself first, which keeps "EchoNote"
//! intact when "Echo" is an alias of something else. ASCII letters compare case-insensitively;
//! an alias that starts or ends with an ASCII letter or digit only matches whole words there.
//! CJK text has no word boundaries and matches exactly.

use std::collections::HashMap;

pub const MAX_ENTRIES: usize = 500;
pub const MAX_ALIASES: usize = 20;
pub const MAX_CHARS: usize = 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: i64,
    pub term: String,
    pub aliases: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GlossaryError {
    #[error("the term is empty")]
    EmptyTerm,
    #[error("a term or alias is longer than {MAX_CHARS} characters")]
    TooLong,
    #[error("a term has more than {MAX_ALIASES} aliases")]
    TooManyAliases,
    #[error("terms and aliases must be on one line")]
    ControlCharacter,
}

/// Trim the term and aliases, drop empty aliases, repeats and aliases equal to the term.
pub fn normalize(term: &str, aliases: &[String]) -> Result<(String, Vec<String>), GlossaryError> {
    let term = check(term)?;
    if term.is_empty() {
        return Err(GlossaryError::EmptyTerm);
    }
    let mut kept: Vec<String> = Vec::new();
    for alias in aliases {
        let alias = check(alias)?;
        if alias.is_empty()
            || alias.eq_ignore_ascii_case(&term)
            || kept.iter().any(|seen| seen.eq_ignore_ascii_case(&alias))
        {
            continue;
        }
        kept.push(alias);
    }
    if kept.len() > MAX_ALIASES {
        return Err(GlossaryError::TooManyAliases);
    }
    Ok((term, kept))
}

fn check(text: &str) -> Result<String, GlossaryError> {
    let text = text.trim();
    if text.chars().any(char::is_control) {
        return Err(GlossaryError::ControlCharacter);
    }
    if text.chars().count() > MAX_CHARS {
        return Err(GlossaryError::TooLong);
    }
    Ok(text.to_owned())
}

struct Pattern {
    text: String,
    /// Index of the entry's term in `Glossary::terms`.
    term: usize,
    /// An alias, replaced by the term; a term matches only to keep its text as it is.
    replace: bool,
    word_start: bool,
    word_end: bool,
}

/// Compiled replacement rules.
#[derive(Default)]
pub struct Glossary {
    terms: Vec<String>,
    aliases: Vec<Vec<String>>,
    /// Patterns by their first character (ASCII lowercased), longest first; a term before
    /// an alias of the same length.
    patterns: HashMap<char, Vec<Pattern>>,
}

fn fold(ch: char) -> char {
    ch.to_ascii_lowercase()
}

fn is_word(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
}

/// Most search spellings tried for one query.
const MAX_VARIANTS: usize = 32;
/// Qwen3-ASR warns past about 48 prompt tokens of hotwords.
const MAX_HOTWORDS: usize = 20;
const MAX_HOTWORD_CHARS: usize = 120;

impl Glossary {
    pub fn new(entries: &[Entry]) -> Self {
        let mut glossary = Glossary::default();
        let mut seen = std::collections::HashSet::new();
        let mut patterns = Vec::new();
        // Terms first: a term that is also another entry's alias stays as it is.
        for (index, entry) in entries.iter().enumerate() {
            glossary.terms.push(entry.term.clone());
            glossary.aliases.push(entry.aliases.clone());
            if seen.insert(entry.term.to_ascii_lowercase()) {
                patterns.push(pattern(&entry.term, index, false));
            }
        }
        for (index, entry) in entries.iter().enumerate() {
            for alias in &entry.aliases {
                if seen.insert(alias.to_ascii_lowercase()) {
                    patterns.push(pattern(alias, index, true));
                }
            }
        }
        // Longest first; equal lengths at one position are the same text, already deduplicated.
        patterns.sort_by_key(|pattern| std::cmp::Reverse(pattern.text.len()));
        for pattern in patterns {
            if let Some(first) = pattern.text.chars().next() {
                glossary
                    .patterns
                    .entry(fold(first))
                    .or_default()
                    .push(pattern);
            }
        }
        glossary
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Terms in the order they were added: hotwords and LLM context.
    pub fn terms(&self) -> &[String] {
        &self.terms
    }

    /// Hotwords for the high-accuracy model: the first terms, comma-separated, short enough
    /// to leave the model's prompt room for 12 s of audio. `None` without terms.
    pub fn hotwords(&self) -> Option<String> {
        let mut out = String::new();
        for term in self.terms.iter().take(MAX_HOTWORDS) {
            let term: String = term
                .chars()
                .filter(|c| *c != ',' && !c.is_control())
                .collect();
            let term = term.trim();
            if term.is_empty() {
                continue;
            }
            if out.chars().count() + term.chars().count() + 1 > MAX_HOTWORD_CHARS {
                break;
            }
            if !out.is_empty() {
                out.push(',');
            }
            out.push_str(term);
        }
        (!out.is_empty()).then_some(out)
    }

    /// `text` with every alias replaced by its term.
    pub fn apply<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        let mut out = String::new();
        let mut copied = 0;
        let mut prev = None;
        let mut at = 0;
        while let Some(ch) = text[at..].chars().next() {
            let Some(found) = self.match_at(text, at, prev) else {
                prev = Some(ch);
                at += ch.len_utf8();
                continue;
            };
            let end = at + found.text.len();
            if found.replace {
                out.push_str(&text[copied..at]);
                out.push_str(&self.terms[found.term]);
                copied = end;
            }
            prev = text[..end].chars().next_back();
            at = end;
        }
        if copied == 0 {
            return std::borrow::Cow::Borrowed(text);
        }
        out.push_str(&text[copied..]);
        std::borrow::Cow::Owned(out)
    }

    /// The longest pattern matching at byte `at`; `prev` is the character before it.
    fn match_at(&self, text: &str, at: usize, prev: Option<char>) -> Option<&Pattern> {
        let rest = &text[at..];
        let candidates = self.patterns.get(&fold(rest.chars().next()?))?;
        candidates.iter().find(|pattern| {
            let len = pattern.text.len();
            // ASCII folding keeps byte lengths and multi-byte characters must match exactly,
            // so `len` is a character boundary of `rest` once the bytes match.
            rest.len() >= len
                && rest.as_bytes()[..len].eq_ignore_ascii_case(pattern.text.as_bytes())
                && !(pattern.word_start && prev.is_some_and(is_word))
                && !(pattern.word_end && rest[len..].chars().next().is_some_and(is_word))
        })
    }

    /// The query and the same query with a term or alias in it swapped for the entry's other
    /// spellings, so searching for a term also finds the recognized text it is shown for.
    pub fn search_variants(&self, query: &str) -> Vec<String> {
        let query = query.trim();
        let lower = query.to_ascii_lowercase();
        let mut variants = vec![query.to_owned()];
        for (term, aliases) in self.terms.iter().zip(&self.aliases) {
            let spellings: Vec<&String> = std::iter::once(term).chain(aliases).collect();
            for found in &spellings {
                let Some(at) = lower.find(&found.to_ascii_lowercase()) else {
                    continue;
                };
                for other in &spellings {
                    let variant = format!("{}{other}{}", &query[..at], &query[at + found.len()..]);
                    if variants.len() < MAX_VARIANTS && !variants.contains(&variant) {
                        variants.push(variant);
                    }
                }
            }
        }
        variants
    }
}

fn pattern(text: &str, term: usize, replace: bool) -> Pattern {
    Pattern {
        text: text.to_owned(),
        term,
        replace,
        word_start: text.chars().next().is_some_and(is_word),
        word_end: text.chars().next_back().is_some_and(is_word),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i64, term: &str, aliases: &[&str]) -> Entry {
        Entry {
            id,
            term: term.to_owned(),
            aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
        }
    }

    fn glossary() -> Glossary {
        Glossary::new(&[
            entry(1, "Kubernetes", &["库伯内特斯", "kubernetis", "K8"]),
            entry(2, "飞书", &["飞鼠", "非书"]),
            entry(3, "飞书文档", &["飞鼠文档"]),
            entry(4, "Echo", &["回声"]),
            entry(5, "EchoNote", &["echo note", "Eco Note"]),
        ])
    }

    #[test]
    fn aliases_become_their_terms() {
        let glossary = glossary();
        assert_eq!(
            glossary.apply("我们用库伯内特斯部署"),
            "我们用Kubernetes部署"
        );
        assert_eq!(glossary.apply("打开飞鼠看一下"), "打开飞书看一下");
        // Longest match first: the document alias wins over the shorter app alias.
        assert_eq!(glossary.apply("发到飞鼠文档里"), "发到飞书文档里");
        assert_eq!(glossary.apply("非书和飞鼠"), "飞书和飞书");
    }

    #[test]
    fn latin_aliases_match_whole_words_in_any_case() {
        let glossary = glossary();
        assert_eq!(
            glossary.apply("deploy on KUBERNETIS today"),
            "deploy on Kubernetes today"
        );
        assert_eq!(glossary.apply("the K8 cluster"), "the Kubernetes cluster");
        // Inside a longer word nothing changes.
        assert_eq!(glossary.apply("K80 and xK8"), "K80 and xK8");
        // CJK next to a Latin alias is a boundary.
        assert_eq!(glossary.apply("用K8部署"), "用Kubernetes部署");
        assert_eq!(glossary.apply("open Eco Note now"), "open EchoNote now");
    }

    #[test]
    fn terms_and_replacements_are_not_matched_again() {
        let glossary = glossary();
        // "Echo" is a term of its own, but "EchoNote" is longer and kept as it is.
        assert_eq!(glossary.apply("EchoNote 和回声"), "EchoNote 和Echo");
        assert_eq!(glossary.apply("echo note"), "EchoNote");
        let once = glossary.apply("库伯内特斯 K8 飞鼠文档").into_owned();
        assert_eq!(glossary.apply(&once), once);
        assert!(matches!(
            glossary.apply("没有别名"),
            std::borrow::Cow::Borrowed(_)
        ));
        assert_eq!(Glossary::new(&[]).apply("飞鼠"), "飞鼠");
    }

    #[test]
    fn a_term_that_is_another_entrys_alias_stays() {
        let glossary = Glossary::new(&[entry(1, "GitHub", &["Git"]), entry(2, "Git", &[])]);
        assert_eq!(glossary.apply("Git and github"), "Git and github");
    }

    #[test]
    fn normalize_trims_and_drops_repeats() {
        let aliases = vec![
            " 飞鼠 ".to_owned(),
            String::new(),
            "飞鼠".to_owned(),
            "飞书".to_owned(),
        ];
        assert_eq!(
            normalize(" 飞书 ", &aliases).unwrap(),
            ("飞书".to_owned(), vec!["飞鼠".to_owned()])
        );
        assert_eq!(normalize("  ", &[]), Err(GlossaryError::EmptyTerm));
        assert_eq!(normalize("a\nb", &[]), Err(GlossaryError::ControlCharacter));
        assert_eq!(
            normalize(&"长".repeat(MAX_CHARS + 1), &[]),
            Err(GlossaryError::TooLong)
        );
        let many: Vec<String> = (0..=MAX_ALIASES).map(|i| format!("a{i}")).collect();
        assert_eq!(normalize("x", &many), Err(GlossaryError::TooManyAliases));
    }

    #[test]
    fn search_variants_cover_every_spelling() {
        let glossary = glossary();
        let variants = glossary.search_variants("kubernetes 集群");
        assert_eq!(variants[0], "kubernetes 集群");
        assert!(variants.contains(&"库伯内特斯 集群".to_owned()));
        assert!(variants.contains(&"K8 集群".to_owned()));
        assert_eq!(glossary.search_variants("预算"), vec!["预算".to_owned()]);
        let short = glossary.search_variants("飞书");
        assert!(short.contains(&"飞鼠".to_owned()) && short.contains(&"非书".to_owned()));
    }

    #[test]
    fn hotwords_are_the_first_terms_without_commas() {
        assert_eq!(Glossary::default().hotwords(), None);
        let entries = [
            entry(1, "飞书", &["飞鼠"]),
            entry(2, "Kubernetes, K8s", &[]),
            entry(3, "Q3", &[]),
        ];
        assert_eq!(
            Glossary::new(&entries).hotwords().as_deref(),
            Some("飞书,Kubernetes K8s,Q3")
        );
        let many: Vec<Entry> = (0..40)
            .map(|i| entry(i, &format!("术语{i}"), &[]))
            .collect();
        let hotwords = Glossary::new(&many).hotwords().unwrap();
        assert_eq!(hotwords.split(',').count(), MAX_HOTWORDS);
        let long: Vec<Entry> = (0..20)
            .map(|i| entry(i, &format!("{i}{}", "长".repeat(30)), &[]))
            .collect();
        assert!(Glossary::new(&long).hotwords().unwrap().chars().count() <= MAX_HOTWORD_CHARS);
    }
}
