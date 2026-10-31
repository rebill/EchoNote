use rusqlite::{Connection, OptionalExtension, params};

use super::{
    Crosstalk, MeetingStatus, NewSegment, NewTrack, Source, Store, StoreError, TrackKind, now_ms,
};

impl Store {
    /// Start a meeting in the `recording` state; returns its id.
    pub fn create_meeting(
        &mut self,
        title: &str,
        started_at_ms: i64,
        language: Option<&str>,
    ) -> Result<i64, StoreError> {
        self.conn.execute(
            "INSERT INTO meetings (title, started_at_ms, status, language)
             VALUES (?1, ?2, 'recording', ?3)",
            params![title, started_at_ms, language],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn add_track(&mut self, meeting: i64, track: &NewTrack) -> Result<i64, StoreError> {
        self.conn.execute(
            "INSERT INTO tracks (meeting_id, kind, file, sample_rate, device_name)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                meeting,
                track.kind,
                track.file,
                track.sample_rate,
                track.device_name
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Store a batch of finals and, in the same transaction, how far each track is now
    /// transcribed, so recovery after a crash resumes exactly there. Returns segment ids.
    pub fn commit_finals(
        &mut self,
        meeting: i64,
        segments: &[NewSegment],
        transcribed_until_ms: &[(TrackKind, i64)],
    ) -> Result<Vec<i64>, StoreError> {
        let now = now_ms();
        let tx = self.conn.transaction()?;
        let mut ids = Vec::with_capacity(segments.len());
        {
            let mut insert = tx.prepare_cached(
                "INSERT INTO segments
                 (meeting_id, track, start_ms, end_ms, text, raw_text, model, source, crosstalk,
                  updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'asr', ?8, ?9)",
            )?;
            for segment in segments {
                insert.execute(params![
                    meeting,
                    segment.track,
                    segment.start_ms,
                    segment.end_ms,
                    segment.text,
                    segment.raw_text,
                    segment.model,
                    segment.crosstalk,
                    now
                ])?;
                ids.push(tx.last_insert_rowid());
            }
            let mut progress = tx.prepare_cached(
                "UPDATE tracks SET transcribed_until_ms = max(transcribed_until_ms, ?3)
                 WHERE meeting_id = ?1 AND kind = ?2",
            )?;
            for (kind, until_ms) in transcribed_until_ms {
                progress.execute(params![meeting, kind, until_ms])?;
            }
        }
        tx.commit()?;
        Ok(ids)
    }

    /// End a recording. The meeting's duration is its longest track, or the wall time when no
    /// track duration is known.
    pub fn finish_meeting(
        &mut self,
        meeting: i64,
        ended_at_ms: i64,
        status: MeetingStatus,
        track_durations_ms: &[(TrackKind, i64)],
    ) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        for (kind, duration_ms) in track_durations_ms {
            tx.execute(
                "UPDATE tracks SET duration_ms = ?3 WHERE meeting_id = ?1 AND kind = ?2",
                params![meeting, kind, duration_ms],
            )?;
        }
        let longest = track_durations_ms
            .iter()
            .map(|(_, duration)| *duration)
            .max();
        let changed = tx.execute(
            "UPDATE meetings
             SET ended_at_ms = ?2, status = ?3, duration_ms = coalesce(?4, ?2 - started_at_ms)
             WHERE id = ?1",
            params![meeting, ended_at_ms, status, longest],
        )?;
        if changed == 0 {
            return Err(StoreError::NoMeeting(meeting));
        }
        tx.commit()?;
        Ok(())
    }

    pub fn rename_meeting(&mut self, meeting: i64, title: &str) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE meetings SET title = ?2 WHERE id = ?1",
            params![meeting, title],
        )?;
        if changed == 0 {
            return Err(StoreError::NoMeeting(meeting));
        }
        Ok(())
    }

    /// Delete a meeting with its tracks, segments, history and index entries.
    pub fn delete_meeting(&mut self, meeting: i64) -> Result<(), StoreError> {
        let changed = self
            .conn
            .execute("DELETE FROM meetings WHERE id = ?1", [meeting])?;
        if changed == 0 {
            return Err(StoreError::NoMeeting(meeting));
        }
        Ok(())
    }

    /// Replace a segment's text; the previous text is kept as a revision.
    pub fn edit_segment(
        &mut self,
        segment: i64,
        text: &str,
        source: Source,
    ) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        edit(&tx, segment, text, source)?;
        tx.commit()?;
        Ok(())
    }

    /// Undo a crosstalk decision the user disagrees with: show a hidden duplicate again, or
    /// put back the words trimmed from it (as a user edit).
    pub fn restore_crosstalk(&mut self, segment: i64) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        let found: Option<(Option<Crosstalk>, Option<String>)> = tx
            .query_row(
                "SELECT crosstalk, raw_text FROM segments WHERE id = ?1",
                [segment],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((crosstalk, raw_text)) = found else {
            return Err(StoreError::NoSegment(segment));
        };
        if let (Some(Crosstalk::Trim), Some(raw_text)) = (crosstalk, raw_text) {
            edit(&tx, segment, &raw_text, Source::User)?;
        }
        tx.execute(
            "UPDATE segments SET crosstalk = NULL WHERE id = ?1",
            [segment],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

/// Archive the current text as a revision and replace it.
fn edit(conn: &Connection, segment: i64, text: &str, source: Source) -> Result<(), StoreError> {
    let archived = conn.execute(
        "INSERT INTO segment_revisions (segment_id, revision, text, source, created_at_ms)
         SELECT id, revision, text, source, updated_at_ms FROM segments WHERE id = ?1",
        [segment],
    )?;
    if archived == 0 {
        return Err(StoreError::NoSegment(segment));
    }
    conn.execute(
        "UPDATE segments
         SET text = ?2, source = ?3, revision = revision + 1, updated_at_ms = ?4
         WHERE id = ?1",
        params![segment, text, source, now_ms()],
    )?;
    Ok(())
}
