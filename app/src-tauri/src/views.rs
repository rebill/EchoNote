//! What the UI receives: rows, notices and live deltas, serialized in camelCase.

use std::borrow::Cow;

use echonote_core::engine::{Notice, SessionStats};
use echonote_core::glossary::Glossary;
use echonote_core::store::{Segment, TrackKind};
use serde::Serialize;

fn track(kind: TrackKind) -> &'static str {
    kind.as_str()
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentView {
    pub id: i64,
    pub track: &'static str,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub crosstalk: Option<&'static str>,
    pub source: &'static str,
    pub revision: u32,
}

impl SegmentView {
    /// The text as shown: glossary aliases replaced by their terms.
    pub fn glossed(mut self, glossary: &Glossary) -> Self {
        if let Cow::Owned(text) = glossary.apply(&self.text) {
            self.text = text;
        }
        self
    }
}

impl From<&Segment> for SegmentView {
    fn from(segment: &Segment) -> Self {
        Self {
            id: segment.id,
            track: track(segment.track),
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            text: segment.text.clone(),
            crosstalk: segment.crosstalk.map(|crosstalk| crosstalk.as_str()),
            source: segment.source.as_str(),
            revision: segment.revision,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NoticeView {
    ModelLoading,
    ModelLoaded { seconds: f64 },
    ModelFailed { error: String },
    SourceStarted { track: &'static str, name: String },
    SourceFailed { track: &'static str, error: String },
    PermissionDenied { track: &'static str },
    DeviceChanged { track: &'static str },
    Bleed,
    AutoStopped,
    StoreFailed { error: String },
    WriteFailed { track: &'static str, error: String },
}

/// What a notice is about; a newer notice replaces the older one on the same subject.
#[derive(Debug, PartialEq, Eq)]
enum Subject {
    Model,
    Source(&'static str),
    Bleed,
    AutoStop,
    Store,
    Write(&'static str),
}

impl NoticeView {
    fn subject(&self) -> Subject {
        match self {
            Self::ModelLoading | Self::ModelLoaded { .. } | Self::ModelFailed { .. } => {
                Subject::Model
            }
            Self::SourceStarted { track, .. }
            | Self::SourceFailed { track, .. }
            | Self::PermissionDenied { track }
            | Self::DeviceChanged { track } => Subject::Source(track),
            Self::Bleed => Subject::Bleed,
            Self::AutoStopped => Subject::AutoStop,
            Self::StoreFailed { .. } => Subject::Store,
            Self::WriteFailed { track, .. } => Subject::Write(track),
        }
    }

    /// Short name for the log (no paths or text).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ModelLoading => "model-loading",
            Self::ModelLoaded { .. } => "model-loaded",
            Self::ModelFailed { .. } => "model-failed",
            Self::SourceStarted { .. } => "source-started",
            Self::SourceFailed { .. } => "source-failed",
            Self::PermissionDenied { .. } => "permission-denied",
            Self::DeviceChanged { .. } => "device-changed",
            Self::Bleed => "bleed",
            Self::AutoStopped => "auto-stopped",
            Self::StoreFailed { .. } => "store-failed",
            Self::WriteFailed { .. } => "write-failed",
        }
    }
}

/// Add `notice` to `notices`, replacing the one on the same subject.
pub fn merge_notice(notices: &mut Vec<NoticeView>, notice: NoticeView) {
    let subject = notice.subject();
    notices.retain(|old| old.subject() != subject);
    notices.push(notice);
}

impl From<&Notice> for NoticeView {
    fn from(notice: &Notice) -> Self {
        match notice {
            Notice::ModelLoading => Self::ModelLoading,
            Notice::ModelLoaded { seconds } => Self::ModelLoaded { seconds: *seconds },
            Notice::ModelFailed { error } => Self::ModelFailed {
                error: error.clone(),
            },
            Notice::SourceStarted { track: kind, name } => Self::SourceStarted {
                track: track(*kind),
                name: name.clone(),
            },
            Notice::SourceFailed { track: kind, error } => Self::SourceFailed {
                track: track(*kind),
                error: error.clone(),
            },
            Notice::PermissionDenied { track: kind } => Self::PermissionDenied {
                track: track(*kind),
            },
            Notice::DeviceChanged { track: kind } => Self::DeviceChanged {
                track: track(*kind),
            },
            Notice::Bleed => Self::Bleed,
            Notice::AutoStopped => Self::AutoStopped,
            Notice::StoreFailed { error } => Self::StoreFailed {
                error: error.clone(),
            },
            Notice::WriteFailed { track: kind, error } => Self::WriteFailed {
                track: track(*kind),
                error: error.clone(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishedView {
    pub duration_ms: i64,
    pub finals: [usize; 2],
    pub hidden: usize,
    pub trimmed: usize,
    pub dropped_frames: u64,
    pub gap_ms: [u64; 2],
    pub spilled_s: f64,
    pub decode_s: f64,
}

impl From<&SessionStats> for FinishedView {
    fn from(stats: &SessionStats) -> Self {
        Self {
            duration_ms: stats.duration_ms,
            finals: stats.finals,
            hidden: stats.hidden,
            trimmed: stats.trimmed,
            dropped_frames: stats.dropped_frames,
            gap_ms: stats.gap_ms,
            spilled_s: stats.spilled_s,
            decode_s: stats.decode_s,
        }
    }
}

/// One batch of live changes for the meeting being recorded. `None` fields did not change.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Delta {
    pub meeting: i64,
    pub state: Option<&'static str>,
    pub elapsed_ms: i64,
    /// Peak level per track (mic, sys) since the last delta, linear.
    pub levels: Option<[f32; 2]>,
    /// Text of the utterance in progress per track (mic, sys); empty when none.
    pub previews: Option<[String; 2]>,
    pub segments: Vec<SegmentView>,
    /// The full current notice list, when it changed.
    pub notices: Option<Vec<NoticeView>>,
    pub finished: Option<FinishedView>,
}

impl Delta {
    /// The delta as shown: glossary applied to segments and previews.
    pub fn glossed(&self, glossary: &Glossary) -> Self {
        let mut delta = self.clone();
        delta.segments = delta
            .segments
            .into_iter()
            .map(|segment| segment.glossed(glossary))
            .collect();
        if let Some(previews) = &mut delta.previews {
            for preview in previews.iter_mut() {
                if let Cow::Owned(text) = glossary.apply(preview) {
                    *preview = text;
                }
            }
        }
        delta
    }

    pub fn new(meeting: i64) -> Self {
        Self {
            meeting,
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.state.is_none()
            && self.levels.is_none()
            && self.previews.is_none()
            && self.segments.is_empty()
            && self.notices.is_none()
            && self.finished.is_none()
    }
}

/// The recording in progress, for a window opened during it. Segments come from the library.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub meeting: i64,
    pub state: &'static str,
    pub elapsed_ms: i64,
    pub previews: [String; 2],
    pub notices: Vec<NoticeView>,
    pub segments: Vec<SegmentView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_notice_replaces_the_one_on_the_same_subject() {
        let mut notices = Vec::new();
        merge_notice(&mut notices, NoticeView::ModelLoading);
        merge_notice(&mut notices, NoticeView::PermissionDenied { track: "sys" });
        merge_notice(&mut notices, NoticeView::ModelLoaded { seconds: 1.5 });
        merge_notice(
            &mut notices,
            NoticeView::SourceStarted {
                track: "mic",
                name: "Built-in".into(),
            },
        );
        assert_eq!(
            notices,
            [
                NoticeView::PermissionDenied { track: "sys" },
                NoticeView::ModelLoaded { seconds: 1.5 },
                NoticeView::SourceStarted {
                    track: "mic",
                    name: "Built-in".into()
                },
            ]
        );
        let json = serde_json::to_value(&notices[0]).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "permissionDenied", "track": "sys"})
        );
        let json = serde_json::to_value(&notices[1]).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "modelLoaded", "seconds": 1.5})
        );
    }

    #[test]
    fn a_glossed_delta_shows_terms_in_segments_and_previews() {
        let glossary = Glossary::new(&[echonote_core::glossary::Entry {
            id: 1,
            term: "飞书".into(),
            aliases: vec!["飞鼠".into()],
        }]);
        let mut delta = Delta::new(3);
        delta.previews = Some(["打开飞鼠".into(), String::new()]);
        delta.segments.push(SegmentView {
            id: 1,
            track: "sys",
            start_ms: 0,
            end_ms: 1,
            text: "飞鼠文档".into(),
            crosstalk: None,
            source: "asr",
            revision: 1,
        });
        let shown = delta.glossed(&glossary);
        assert_eq!(shown.segments[0].text, "飞书文档");
        assert_eq!(shown.previews, Some(["打开飞书".to_owned(), String::new()]));
        assert_eq!(
            delta.segments[0].text, "飞鼠文档",
            "the original is left alone"
        );
    }
}
