//! Segments transcribed again with the high-accuracy model. Each pass changes text through
//! the revision history; undo restores the text and model each segment had before the
//! first pass, except where the user changed it since.

use rusqlite::{OptionalExtension, params};

use super::write::edit;
use super::{Source, Store, StoreError};

/// The high-accuracy model's text for a segment, decoded from `revision`.
#[derive(Clone, Debug, PartialEq)]
pub struct Retranscribed {
    pub segment: i64,
    pub revision: u32,
    pub text: String,
}

impl Store {
    /// Apply one batch of a pass by `model`. A segment that changed since it was read (a
    /// new revision or a text that is no longer the recognizer's) is skipped. Returns how
    /// many were applied.
    pub fn apply_retranscription(
        &mut self,
        meeting: i64,
        model: &str,
        batch: &[Retranscribed],
    ) -> Result<usize, StoreError> {
        let tx = self.conn.transaction()?;
        let mut applied = 0;
        for item in batch {
            let current: Option<(i64, u32, Source, String, String)> = tx
                .query_row(
                    "SELECT meeting_id, revision, source, model, text FROM segments WHERE id = ?1",
                    [item.segment],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .optional()?;
            let Some((owner, revision, source, previous_model, text)) = current else {
                continue;
            };
            if owner != meeting || revision != item.revision || source != Source::Asr {
                continue;
            }
            // A row from a pass the user has since edited over no longer describes the
            // segment: start again from what it is now.
            tx.execute(
                "DELETE FROM retranscriptions WHERE segment_id = ?1 AND revision != ?2",
                params![item.segment, revision],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO retranscriptions
                     (segment_id, meeting_id, previous_revision, previous_model, revision)
                 VALUES (?1, ?2, ?3, ?4, ?3)",
                params![item.segment, meeting, revision, previous_model],
            )?;
            if item.text != text {
                edit(&tx, item.segment, &item.text, Source::Asr)?;
            }
            tx.execute(
                "UPDATE segments SET model = ?2 WHERE id = ?1",
                params![item.segment, model],
            )?;
            tx.execute(
                "UPDATE retranscriptions
                 SET revision = (SELECT revision FROM segments WHERE id = ?1)
                 WHERE segment_id = ?1",
                [item.segment],
            )?;
            applied += 1;
        }
        tx.commit()?;
        Ok(applied)
    }

    /// Segments of `meeting` a pass changed that the user has not touched since.
    pub fn retranscribed(&self, meeting: i64) -> Result<usize, StoreError> {
        let count: i64 = self.conn.query_row(
            "SELECT count(*) FROM retranscriptions r JOIN segments s ON s.id = r.segment_id
             WHERE r.meeting_id = ?1 AND s.revision = r.revision",
            [meeting],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Segments of `meeting` whose current revision is a pass's result. Any later change
    /// (an edit, a restored revision) makes a segment due for the next pass again.
    pub fn retranscribed_segments(&self, meeting: i64) -> Result<Vec<i64>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT r.segment_id FROM retranscriptions r JOIN segments s ON s.id = r.segment_id
             WHERE r.meeting_id = ?1 AND s.revision = r.revision",
        )?;
        let rows = statement.query_map([meeting], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Undo every pass on `meeting`: each segment gets back the text and model it had
    /// before the first pass, as a new revision. Segments the user changed since keep
    /// their text. Returns how many were restored.
    pub fn revert_retranscription(&mut self, meeting: i64) -> Result<usize, StoreError> {
        let tx = self.conn.transaction()?;
        let rows: Vec<(i64, u32, String, u32)> = {
            let mut statement = tx.prepare(
                "SELECT segment_id, previous_revision, previous_model, revision
                 FROM retranscriptions WHERE meeting_id = ?1 ORDER BY segment_id",
            )?;
            let rows = statement.query_map([meeting], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?;
            rows.collect::<Result<_, _>>()?
        };
        let mut restored = 0;
        for (segment, previous, model, revision) in rows {
            let current: Option<u32> = tx
                .query_row(
                    "SELECT revision FROM segments WHERE id = ?1",
                    [segment],
                    |row| row.get(0),
                )
                .optional()?;
            if current != Some(revision) {
                continue;
            }
            if previous != revision {
                let (text, source): (String, Source) = tx.query_row(
                    "SELECT text, source FROM segment_revisions
                     WHERE segment_id = ?1 AND revision = ?2",
                    params![segment, previous],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                edit(&tx, segment, &text, source)?;
            }
            tx.execute(
                "UPDATE segments SET model = ?2 WHERE id = ?1",
                params![segment, model],
            )?;
            restored += 1;
        }
        tx.execute(
            "DELETE FROM retranscriptions WHERE meeting_id = ?1",
            [meeting],
        )?;
        tx.commit()?;
        Ok(restored)
    }
}
