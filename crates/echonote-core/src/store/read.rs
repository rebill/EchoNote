use rusqlite::{OptionalExtension, Row, params};

use super::{Meeting, Revision, Segment, Store, StoreError, Track, TrackKind};

/// The trigram index needs at least this many characters; shorter queries scan with LIKE.
const MIN_INDEXED_QUERY: usize = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub meeting_id: i64,
    pub meeting_title: String,
    pub segment_id: i64,
    pub track: TrackKind,
    pub start_ms: i64,
    pub text: String,
}

const MEETING_COLUMNS: &str =
    "id, title, started_at_ms, ended_at_ms, duration_ms, status, language";
const TRACK_COLUMNS: &str = "id, meeting_id, kind, file, sample_rate, device_name, duration_ms, \
     transcribed_until_ms";
const SEGMENT_COLUMNS: &str = "id, meeting_id, track, start_ms, end_ms, text, raw_text, model, \
     source, crosstalk, revision";

impl Store {
    /// All meetings, newest first.
    pub fn meetings(&self) -> Result<Vec<Meeting>, StoreError> {
        let mut statement = self.conn.prepare_cached(&format!(
            "SELECT {MEETING_COLUMNS} FROM meetings ORDER BY started_at_ms DESC, id DESC"
        ))?;
        let rows = statement.query_map([], meeting)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn meeting(&self, id: i64) -> Result<Meeting, StoreError> {
        self.conn
            .query_row(
                &format!("SELECT {MEETING_COLUMNS} FROM meetings WHERE id = ?1"),
                [id],
                meeting,
            )
            .optional()?
            .ok_or(StoreError::NoMeeting(id))
    }

    /// Meetings still marked `recording`: the app stopped without finishing them.
    pub fn unfinished_meetings(&self) -> Result<Vec<Meeting>, StoreError> {
        let mut statement = self.conn.prepare_cached(&format!(
            "SELECT {MEETING_COLUMNS} FROM meetings WHERE status = 'recording'
             ORDER BY started_at_ms"
        ))?;
        let rows = statement.query_map([], meeting)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn tracks(&self, meeting_id: i64) -> Result<Vec<Track>, StoreError> {
        let mut statement = self.conn.prepare_cached(&format!(
            "SELECT {TRACK_COLUMNS} FROM tracks WHERE meeting_id = ?1 ORDER BY kind"
        ))?;
        let rows = statement.query_map([meeting_id], track)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Segments of a meeting in time order, crosstalk duplicates included.
    pub fn segments(&self, meeting_id: i64) -> Result<Vec<Segment>, StoreError> {
        let mut statement = self.conn.prepare_cached(&format!(
            "SELECT {SEGMENT_COLUMNS} FROM segments WHERE meeting_id = ?1
             ORDER BY start_ms, track"
        ))?;
        let rows = statement.query_map([meeting_id], segment)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn segment(&self, id: i64) -> Result<Segment, StoreError> {
        self.conn
            .query_row(
                &format!("SELECT {SEGMENT_COLUMNS} FROM segments WHERE id = ?1"),
                [id],
                segment,
            )
            .optional()?
            .ok_or(StoreError::NoSegment(id))
    }

    /// Earlier texts of a segment, oldest first.
    pub fn revisions(&self, segment_id: i64) -> Result<Vec<Revision>, StoreError> {
        let mut statement = self.conn.prepare_cached(
            "SELECT revision, text, source, created_at_ms FROM segment_revisions
             WHERE segment_id = ?1 ORDER BY revision",
        )?;
        let rows = statement.query_map([segment_id], |row| {
            Ok(Revision {
                revision: row.get(0)?,
                text: row.get(1)?,
                source: row.get(2)?,
                created_at_ms: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// Visible segments containing `query` as a substring (case-insensitive), newest meeting
    /// first and in time order within a meeting. `meeting` limits the search to one meeting.
    pub fn search(
        &self,
        query: &str,
        meeting: Option<i64>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let (filter, pattern) = if query.chars().count() >= MIN_INDEXED_QUERY {
            (
                "s.id IN (SELECT rowid FROM segments_fts WHERE segments_fts MATCH ?1)",
                format!("\"{}\"", query.replace('"', "\"\"")),
            )
        } else {
            (
                "s.crosstalk IS NOT 'duplicate' AND s.text LIKE ?1 ESCAPE '\\'",
                format!("%{}%", escape_like(query)),
            )
        };
        let sql = format!(
            "SELECT s.meeting_id, m.title, s.id, s.track, s.start_ms, s.text
             FROM segments s JOIN meetings m ON m.id = s.meeting_id
             WHERE {filter} AND (?2 IS NULL OR s.meeting_id = ?2)
             ORDER BY m.started_at_ms DESC, s.meeting_id DESC, s.start_ms
             LIMIT ?3"
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let rows = statement.query_map(params![pattern, meeting, limit as i64], |row| {
            Ok(SearchHit {
                meeting_id: row.get(0)?,
                meeting_title: row.get(1)?,
                segment_id: row.get(2)?,
                track: row.get(3)?,
                start_ms: row.get(4)?,
                text: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn meeting(row: &Row<'_>) -> rusqlite::Result<Meeting> {
    Ok(Meeting {
        id: row.get(0)?,
        title: row.get(1)?,
        started_at_ms: row.get(2)?,
        ended_at_ms: row.get(3)?,
        duration_ms: row.get(4)?,
        status: row.get(5)?,
        language: row.get(6)?,
    })
}

fn track(row: &Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        kind: row.get(2)?,
        file: row.get(3)?,
        sample_rate: row.get(4)?,
        device_name: row.get(5)?,
        duration_ms: row.get(6)?,
        transcribed_until_ms: row.get(7)?,
    })
}

fn segment(row: &Row<'_>) -> rusqlite::Result<Segment> {
    Ok(Segment {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        track: row.get(2)?,
        start_ms: row.get(3)?,
        end_ms: row.get(4)?,
        text: row.get(5)?,
        raw_text: row.get(6)?,
        model: row.get(7)?,
        source: row.get(8)?,
        crosstalk: row.get(9)?,
        revision: row.get(10)?,
    })
}
