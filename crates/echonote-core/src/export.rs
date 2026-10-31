//! A meeting as Markdown or plain text. The Markdown keeps the old plugin's note layout
//! (title, meta lines, then `[mm:ss] 我：text` transcript lines) so existing Obsidian notes and
//! the plugin's transcript parser read it the same way. The glossary is applied; lines hidden
//! as speaker bleed are left out; speaker labels appear only when the other side spoke.
//!
//! Times are local: the caller passes the UTC offset that applied when the meeting started.

use crate::glossary::Glossary;
use crate::store::{Crosstalk, Meeting, Segment, TrackKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Text,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Markdown => "md",
            Format::Text => "txt",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Zh,
    En,
}

struct Labels {
    me: &'static str,
    other: &'static str,
    colon: &'static str,
    date: &'static str,
    time: &'static str,
    duration: &'static str,
    transcript: &'static str,
    tags: &'static str,
    untitled: &'static str,
}

fn labels(language: Language) -> &'static Labels {
    match language {
        Language::Zh => &Labels {
            me: "我",
            other: "对方",
            colon: "：",
            date: "日期",
            time: "时间",
            duration: "时长",
            transcript: "转录",
            tags: "标签",
            untitled: "的会议",
        },
        Language::En => &Labels {
            me: "Me",
            other: "Other",
            colon: ": ",
            date: "Date",
            time: "Time",
            duration: "Duration",
            transcript: "Transcript",
            tags: "Tags",
            untitled: "Meeting on",
        },
    }
}

/// Offsets beyond these are not real time zones.
pub const MAX_OFFSET_MINUTES: i32 = 14 * 60;

pub struct Export<'a> {
    pub meeting: &'a Meeting,
    pub segments: &'a [Segment],
    pub glossary: &'a Glossary,
    pub language: Language,
    /// Minutes east of UTC when the meeting started (UTC+8 is 480).
    pub utc_offset_minutes: i32,
    /// The latest notes (Markdown sections), placed before the transcript.
    pub notes: Option<&'a str>,
}

/// A transcript line in the text format, `[mm:ss] 我：text`, with its segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptLine {
    pub segment: i64,
    pub start_ms: i64,
    pub text: String,
}

/// Local calendar time: (year, month, day, hour, minute).
fn local(ms: i64, offset_minutes: i32) -> (i64, u32, u32, u32, u32) {
    let seconds = ms.div_euclid(1000) + i64::from(offset_minutes) * 60;
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year,
        month,
        day,
        (of_day / 3600) as u32,
        (of_day % 3600 / 60) as u32,
    )
}

/// `mm:ss`, or `hh:mm:ss` from one hour (the plugin's transcript timestamps).
fn timestamp(ms: i64) -> String {
    let total = ms.max(0) / 1000;
    let (hours, minutes, seconds) = (total / 3600, total % 3600 / 60, total % 60);
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn duration(ms: i64, language: Language) -> String {
    let minutes = ms.max(0) / 60_000;
    let (hours, rest) = (minutes / 60, minutes % 60);
    match (language, hours, rest) {
        (Language::Zh, _, _) if minutes < 1 => "< 1 分钟".into(),
        (Language::En, _, _) if minutes < 1 => "< 1 min".into(),
        (Language::Zh, 0, _) => format!("{rest} 分钟"),
        (Language::En, 0, _) => format!("{rest} min"),
        (Language::Zh, _, 0) => format!("{hours} 小时"),
        (Language::En, _, 0) => format!("{hours} h"),
        (Language::Zh, _, _) => format!("{hours} 小时 {rest} 分钟"),
        (Language::En, _, _) => format!("{hours} h {rest} min"),
    }
}

/// One line: runs of whitespace, including line breaks typed into an edit, become a space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Backslash-escape what Markdown or Obsidian would turn into formatting, links, tags, HTML
/// or math, so recognized text shows as typed.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(
            ch,
            '\\' | '`'
                | '*'
                | '_'
                | '['
                | ']'
                | '<'
                | '>'
                | '|'
                | '~'
                | '$'
                | '#'
                | '='
                | '%'
                | '^'
                | '&'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Characters not allowed in file names on Windows or macOS.
fn safe_file_name(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            {
                '_'
            } else {
                ch
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    trimmed
        .chars()
        .take(80)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

impl Export<'_> {
    fn date_time(&self, ms: i64) -> (String, String) {
        let (year, month, day, hour, minute) = local(ms, self.utc_offset_minutes);
        (
            format!("{year:04}-{month:02}-{day:02}"),
            format!("{hour:02}:{minute:02}"),
        )
    }

    /// The meeting's title, or "<date time> 的会议" when it has none (as in the app).
    pub fn title(&self) -> String {
        let title = one_line(&self.meeting.title);
        if !title.is_empty() {
            return title;
        }
        let (date, time) = self.date_time(self.meeting.started_at_ms);
        match self.language {
            Language::Zh => format!("{date} {time} {}", labels(self.language).untitled),
            Language::En => format!("{} {date} {time}", labels(self.language).untitled),
        }
    }

    /// "<title> <date>.md", or "EchoNote <date> <hhmm>.md" for an untitled meeting.
    pub fn file_name(&self, format: Format) -> String {
        let (date, time) = self.date_time(self.meeting.started_at_ms);
        let title = safe_file_name(&one_line(&self.meeting.title));
        let stem = if title.is_empty() {
            format!("EchoNote {date} {}", time.replace(':', ""))
        } else {
            format!("{title} {date}")
        };
        format!("{stem}.{}", format.extension())
    }

    /// Transcript lines: segment, start, speaker label, text.
    fn lines(&self) -> Vec<(i64, i64, Option<&'static str>, String)> {
        let labels = labels(self.language);
        let shown = |segment: &&Segment| segment.crosstalk != Some(Crosstalk::Duplicate);
        let other_spoke = self
            .segments
            .iter()
            .filter(shown)
            .any(|segment| segment.track == TrackKind::Sys && !segment.text.trim().is_empty());
        self.segments
            .iter()
            .filter(shown)
            .filter_map(|segment| {
                let text = one_line(&self.glossary.apply(&segment.text));
                if text.is_empty() {
                    return None;
                }
                let speaker = other_spoke.then_some(match segment.track {
                    TrackKind::Mic => labels.me,
                    TrackKind::Sys => labels.other,
                });
                Some((segment.id, segment.start_ms, speaker, text))
            })
            .collect()
    }

    fn line(
        &self,
        (_, start_ms, speaker, _): &(i64, i64, Option<&str>, String),
        text: String,
    ) -> String {
        let colon = labels(self.language).colon;
        match speaker {
            Some(speaker) => format!("[{}] {speaker}{colon}{text}", timestamp(*start_ms)),
            None => format!("[{}] {text}", timestamp(*start_ms)),
        }
    }

    /// The transcript as the text export shows it, line by line (what the LLM reads).
    pub fn transcript(&self) -> Vec<TranscriptLine> {
        self.lines()
            .into_iter()
            .map(|entry| TranscriptLine {
                segment: entry.0,
                start_ms: entry.1,
                text: self.line(&entry, entry.3.clone()),
            })
            .collect()
    }

    pub fn render(&self, format: Format) -> String {
        let labels = labels(self.language);
        let colon = labels.colon;
        let (date, start) = self.date_time(self.meeting.started_at_ms);
        let time = match self.meeting.ended_at_ms {
            Some(ended) => format!("{start} - {}", self.date_time(ended).1),
            None => start,
        };
        let length = self
            .meeting
            .duration_ms
            .map(|ms| duration(ms, self.language));
        let line = |entry: &(i64, i64, Option<&str>, String), escaped: bool| {
            self.line(
                entry,
                if escaped {
                    escape(&entry.3)
                } else {
                    entry.3.clone()
                },
            )
        };
        let notes = self.notes.map(str::trim).filter(|notes| !notes.is_empty());
        let mut out = Vec::new();
        match format {
            Format::Markdown => {
                out.push(format!("# {}", escape(&self.title())));
                out.push(String::new());
                out.push(format!("- {}{colon}{date}", labels.date));
                out.push(format!("- {}{colon}{time}", labels.time));
                if let Some(length) = length {
                    out.push(format!("- {}{colon}{length}", labels.duration));
                }
                out.push(format!("- {}{colon}#meeting #echonote", labels.tags));
                out.push(String::new());
                if let Some(notes) = notes {
                    out.push(notes.to_owned());
                    out.push(String::new());
                }
                out.push(format!("## {}", labels.transcript));
                // A blank line between lines: each is its own paragraph in any renderer.
                for entry in self.lines() {
                    out.push(String::new());
                    out.push(line(&entry, true));
                }
            }
            Format::Text => {
                out.push(self.title());
                match length {
                    Some(length) => out.push(format!("{date} {time} · {length}")),
                    None => out.push(format!("{date} {time}")),
                }
                out.push(String::new());
                if let Some(notes) = notes {
                    out.push(notes.to_owned());
                    out.push(String::new());
                    out.push(labels.transcript.to_owned());
                }
                out.extend(self.lines().iter().map(|entry| line(entry, false)));
            }
        }
        let mut text = out.join("\n");
        text.push('\n');
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glossary::Entry;
    use crate::store::{MeetingStatus, Source};

    // 2026-10-31 14:00 at UTC+8.
    const STARTED: i64 = 1_793_426_400_000;

    fn meeting(title: &str) -> Meeting {
        Meeting {
            id: 1,
            title: title.into(),
            started_at_ms: STARTED,
            ended_at_ms: Some(STARTED + 3_725_000),
            duration_ms: Some(3_725_000),
            status: MeetingStatus::Finalized,
            language: Some("zh".into()),
        }
    }

    fn segment(id: i64, track: TrackKind, start_ms: i64, text: &str) -> Segment {
        Segment {
            id,
            meeting_id: 1,
            track,
            start_ms,
            end_ms: start_ms + 1_000,
            text: text.into(),
            raw_text: None,
            model: "test".into(),
            source: Source::Asr,
            crosstalk: None,
            revision: 1,
        }
    }

    fn segments() -> Vec<Segment> {
        vec![
            segment(1, TrackKind::Mic, 5_000, "打开飞鼠看一下"),
            Segment {
                crosstalk: Some(Crosstalk::Duplicate),
                ..segment(2, TrackKind::Mic, 6_000, "回声")
            },
            segment(3, TrackKind::Sys, 7_000, "好的 *重点*\n第二行"),
            segment(4, TrackKind::Mic, 3_725_000, "散会"),
        ]
    }

    fn glossary() -> Glossary {
        Glossary::new(&[Entry {
            id: 1,
            term: "飞书".into(),
            aliases: vec!["飞鼠".into()],
        }])
    }

    #[test]
    fn local_dates_follow_the_offset() {
        assert_eq!(local(0, 0), (1970, 1, 1, 0, 0));
        assert_eq!(local(0, 480), (1970, 1, 1, 8, 0));
        assert_eq!(local(-1, 0), (1969, 12, 31, 23, 59));
        assert_eq!(local(951_782_400_000, 0), (2000, 2, 29, 0, 0));
        assert_eq!(local(STARTED, 480), (2026, 10, 31, 14, 0));
        assert_eq!(local(STARTED, -300), (2026, 10, 31, 1, 0));
    }

    #[test]
    fn markdown_keeps_the_plugin_layout() {
        let meeting = meeting("周会 #1");
        let (segments, glossary) = (segments(), glossary());
        let export = Export {
            meeting: &meeting,
            segments: &segments,
            glossary: &glossary,
            language: Language::Zh,
            utc_offset_minutes: 480,
            notes: None,
        };
        assert_eq!(
            export.render(Format::Markdown),
            "# 周会 \\#1\n\n- 日期：2026-10-31\n- 时间：14:00 - 15:02\n- 时长：1 小时 2 分钟\n\
             - 标签：#meeting #echonote\n\n## 转录\n\n[00:05] 我：打开飞书看一下\n\n\
             [00:07] 对方：好的 \\*重点\\* 第二行\n\n[01:02:05] 我：散会\n"
        );
        assert_eq!(export.file_name(Format::Markdown), "周会 #1 2026-10-31.md");
    }

    #[test]
    fn text_has_no_markup_and_labels_only_when_the_other_side_spoke() {
        let meeting = meeting("a/b: c?");
        let segments: Vec<Segment> = segments()
            .into_iter()
            .filter(|segment| segment.track == TrackKind::Mic)
            .collect();
        let glossary = Glossary::new(&[]);
        let export = Export {
            meeting: &meeting,
            segments: &segments,
            glossary: &glossary,
            language: Language::En,
            utc_offset_minutes: 480,
            notes: None,
        };
        assert_eq!(
            export.render(Format::Text),
            "a/b: c?\n2026-10-31 14:00 - 15:02 · 1 h 2 min\n\n[00:05] 打开飞鼠看一下\n[01:02:05] 散会\n"
        );
        assert_eq!(export.file_name(Format::Text), "a_b_ c_ 2026-10-31.txt");
    }

    /// Text that would become formatting, links, tags, HTML or math stays literal.
    /// `ECHONOTE_EXPORT_SAMPLE=<path>` writes the Markdown out for a renderer check.
    #[test]
    fn markup_in_the_transcript_is_escaped() {
        let meeting = meeting("*Plan* [v2]");
        let text =
            "**加粗** _斜体_ [链接](x) [[笔记]] #标签 <b>粗</b> $x$ ==高亮== `代码` a\\b ~~删~~";
        let segments = [segment(1, TrackKind::Mic, 0, text)];
        let export = Export {
            meeting: &meeting,
            segments: &segments,
            glossary: &Glossary::new(&[]),
            language: Language::Zh,
            utc_offset_minutes: 480,
            notes: None,
        };
        let markdown = export.render(Format::Markdown);
        assert!(markdown.starts_with("# \\*Plan\\* \\[v2\\]\n"));
        assert!(markdown.ends_with(
            "[00:00] \\*\\*加粗\\*\\* \\_斜体\\_ \\[链接\\](x) \\[\\[笔记\\]\\] \\#标签 \
             \\<b\\>粗\\</b\\> \\$x\\$ \\=\\=高亮\\=\\= \\`代码\\` a\\\\b \\~\\~删\\~\\~\n"
        ));
        assert!(export.render(Format::Text).contains(text));
        if let Some(path) = std::env::var_os("ECHONOTE_EXPORT_SAMPLE") {
            std::fs::write(path, markdown).unwrap();
        }
    }

    #[test]
    fn notes_go_before_the_transcript_and_lines_keep_their_segments() {
        let meeting = meeting("周会");
        let (segments, glossary) = (segments(), glossary());
        let export = Export {
            meeting: &meeting,
            segments: &segments,
            glossary: &glossary,
            language: Language::Zh,
            utc_offset_minutes: 480,
            notes: Some("## 概要\n讨论了飞书。\n"),
        };
        let markdown = export.render(Format::Markdown);
        assert!(
            markdown.contains("- 标签：#meeting #echonote\n\n## 概要\n讨论了飞书。\n\n## 转录\n")
        );
        assert!(
            export
                .render(Format::Text)
                .contains("· 1 小时 2 分钟\n\n## 概要\n讨论了飞书。\n\n转录\n[00:05]")
        );
        let lines = export.transcript();
        assert_eq!(lines.len(), 3, "the hidden line is left out");
        assert_eq!(
            lines[0],
            TranscriptLine {
                segment: 1,
                start_ms: 5_000,
                text: "[00:05] 我：打开飞书看一下".into()
            }
        );
        assert_eq!(lines[1].text, "[00:07] 对方：好的 *重点* 第二行");
    }

    #[test]
    fn an_untitled_meeting_is_named_by_its_start() {
        let meeting = meeting("  ");
        let export = Export {
            meeting: &meeting,
            segments: &[],
            glossary: &Glossary::new(&[]),
            language: Language::Zh,
            utc_offset_minutes: 480,
            notes: None,
        };
        assert_eq!(export.title(), "2026-10-31 14:00 的会议");
        assert_eq!(
            export.file_name(Format::Markdown),
            "EchoNote 2026-10-31 1400.md"
        );
        assert!(export.render(Format::Markdown).ends_with("## 转录\n"));
    }
}
