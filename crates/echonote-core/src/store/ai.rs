//! AI results kept with a meeting: notes and live highlights, Q&A turns, the user's summary
//! templates, and LLM corrections awaiting review. All of it goes with the meeting when it
//! is deleted (PRD A12).

use rusqlite::{OptionalExtension, Row, params};

use super::write::edit;
use super::{Source, Store, StoreError, SummaryKind, now_ms};

pub const MAX_TEMPLATES: usize = 50;
pub const MAX_TEMPLATE_NAME: usize = 60;
pub const MAX_TEMPLATE_CHARS: usize = 4_000;

#[derive(Clone, Debug, PartialEq)]
pub struct SummaryRow {
    pub id: i64,
    pub meeting_id: i64,
    pub kind: SummaryKind,
    pub template: String,
    /// Who wrote it, as shown: "<provider> / <model>".
    pub provider: String,
    pub title: Option<String>,
    pub content: String,
    pub truncated: bool,
    pub created_at_ms: i64,
}

pub struct NewSummary<'a> {
    pub kind: SummaryKind,
    pub template: &'a str,
    pub provider: &'a str,
    pub title: Option<&'a str>,
    pub content: &'a str,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QaTurn {
    pub id: i64,
    pub question: String,
    pub answer: String,
    pub provider: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TemplateRow {
    pub id: i64,
    pub name: String,
    pub instructions: String,
}

/// An LLM change to apply, made against `revision` of the segment.
#[derive(Clone, Debug, PartialEq)]
pub struct NewCorrection {
    pub segment: i64,
    pub revision: u32,
    pub text: String,
}

/// An LLM change awaiting review, with the text it replaced.
#[derive(Clone, Debug, PartialEq)]
pub struct Correction {
    pub segment_id: i64,
    pub previous_revision: u32,
    pub previous_text: String,
}

/// A foreign key failure means the meeting is gone.
fn meeting_error(error: rusqlite::Error, meeting: i64) -> StoreError {
    match &error {
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY =>
        {
            StoreError::NoMeeting(meeting)
        }
        _ => error.into(),
    }
}

fn summary_row(row: &Row<'_>) -> rusqlite::Result<SummaryRow> {
    Ok(SummaryRow {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        kind: row.get(2)?,
        template: row.get(3)?,
        provider: row.get(4)?,
        title: row.get(5)?,
        content: row.get(6)?,
        truncated: row.get(7)?,
        created_at_ms: row.get(8)?,
    })
}

impl Store {
    /// Store notes, or replace the meeting's highlights.
    pub fn save_summary(
        &mut self,
        meeting: i64,
        summary: &NewSummary<'_>,
    ) -> Result<i64, StoreError> {
        let tx = self.conn.transaction()?;
        if summary.kind == SummaryKind::Highlights {
            tx.execute(
                "DELETE FROM summaries WHERE meeting_id = ?1 AND kind = 'highlights'",
                [meeting],
            )?;
        }
        tx.execute(
            "INSERT INTO summaries
             (meeting_id, kind, template, provider, title, content, truncated, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                meeting,
                summary.kind,
                summary.template,
                summary.provider,
                summary.title,
                summary.content,
                summary.truncated,
                now_ms()
            ],
        )
        .map_err(|error| meeting_error(error, meeting))?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    pub fn latest_summary(
        &self,
        meeting: i64,
        kind: SummaryKind,
    ) -> Result<Option<SummaryRow>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, meeting_id, kind, template, provider, title, content, truncated,
                        created_at_ms
                 FROM summaries WHERE meeting_id = ?1 AND kind = ?2
                 ORDER BY created_at_ms DESC, id DESC LIMIT 1",
                params![meeting, kind],
                summary_row,
            )
            .optional()?)
    }

    pub fn add_qa(
        &mut self,
        meeting: i64,
        question: &str,
        answer: &str,
        provider: &str,
    ) -> Result<i64, StoreError> {
        self.conn
            .execute(
                "INSERT INTO qa (meeting_id, question, answer, provider, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![meeting, question, answer, provider, now_ms()],
            )
            .map_err(|error| meeting_error(error, meeting))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Question and answer turns, oldest first.
    pub fn qa(&self, meeting: i64) -> Result<Vec<QaTurn>, StoreError> {
        let mut statement = self.conn.prepare_cached(
            "SELECT id, question, answer, provider, created_at_ms FROM qa
             WHERE meeting_id = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map([meeting], |row| {
            Ok(QaTurn {
                id: row.get(0)?,
                question: row.get(1)?,
                answer: row.get(2)?,
                provider: row.get(3)?,
                created_at_ms: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn clear_qa(&mut self, meeting: i64) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM qa WHERE meeting_id = ?1", [meeting])?;
        Ok(())
    }
}
fn check_template(name: &str, instructions: &str) -> Result<(String, String), StoreError> {
    let (name, instructions) = (name.trim(), instructions.trim());
    let bad = name.is_empty()
        || instructions.is_empty()
        || name.chars().any(char::is_control)
        || name.chars().count() > MAX_TEMPLATE_NAME
        || instructions.chars().count() > MAX_TEMPLATE_CHARS;
    if bad {
        return Err(StoreError::InvalidTemplate);
    }
    Ok((name.to_owned(), instructions.to_owned()))
}

impl Store {
    pub fn templates(&self) -> Result<Vec<TemplateRow>, StoreError> {
        let mut statement = self
            .conn
            .prepare_cached("SELECT id, name, instructions FROM templates ORDER BY id")?;
        let rows = statement.query_map([], |row| {
            Ok(TemplateRow {
                id: row.get(0)?,
                name: row.get(1)?,
                instructions: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn template(&self, id: i64) -> Result<TemplateRow, StoreError> {
        self.conn
            .query_row(
                "SELECT id, name, instructions FROM templates WHERE id = ?1",
                [id],
                |row| {
                    Ok(TemplateRow {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        instructions: row.get(2)?,
                    })
                },
            )
            .optional()?
            .ok_or(StoreError::NoTemplate(id))
    }

    /// Add a template (`id` None) or replace one; returns its id.
    pub fn save_template(
        &mut self,
        id: Option<i64>,
        name: &str,
        instructions: &str,
    ) -> Result<i64, StoreError> {
        let (name, instructions) = check_template(name, instructions)?;
        match id {
            None => {
                let count: i64 =
                    self.conn
                        .query_row("SELECT count(*) FROM templates", [], |row| row.get(0))?;
                if count as usize >= MAX_TEMPLATES {
                    return Err(StoreError::TooManyTemplates);
                }
                self.conn.execute(
                    "INSERT INTO templates (name, instructions, updated_at_ms) VALUES (?1, ?2, ?3)",
                    params![name, instructions, now_ms()],
                )?;
                Ok(self.conn.last_insert_rowid())
            }
            Some(id) => {
                let changed = self.conn.execute(
                    "UPDATE templates SET name = ?2, instructions = ?3, updated_at_ms = ?4
                     WHERE id = ?1",
                    params![id, name, instructions, now_ms()],
                )?;
                if changed == 0 {
                    return Err(StoreError::NoTemplate(id));
                }
                Ok(id)
            }
        }
    }

    pub fn delete_template(&mut self, id: i64) -> Result<(), StoreError> {
        let changed = self
            .conn
            .execute("DELETE FROM templates WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(StoreError::NoTemplate(id));
        }
        Ok(())
    }

    /// Apply LLM changes as revisions and put them up for review. A change made against an
    /// older revision than the segment has now (the user edited it meanwhile) is skipped.
    /// A segment corrected twice keeps the revision before the first correction, so revert
    /// goes all the way back. Returns how many were applied.
    pub fn apply_corrections(
        &mut self,
        meeting: i64,
        corrections: &[NewCorrection],
    ) -> Result<usize, StoreError> {
        let tx = self.conn.transaction()?;
        let mut applied = 0;
        for correction in corrections {
            let current: Option<(i64, u32)> = tx
                .query_row(
                    "SELECT meeting_id, revision FROM segments WHERE id = ?1",
                    [correction.segment],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if current != Some((meeting, correction.revision)) {
                continue;
            }
            tx.execute(
                "INSERT OR IGNORE INTO llm_corrections (segment_id, meeting_id, previous_revision)
                 VALUES (?1, ?2, ?3)",
                params![correction.segment, meeting, correction.revision],
            )?;
            edit(&tx, correction.segment, &correction.text, Source::Llm)?;
            applied += 1;
        }
        tx.commit()?;
        Ok(applied)
    }

    /// LLM changes in `meeting` the user has not reviewed.
    pub fn corrections(&self, meeting: i64) -> Result<Vec<Correction>, StoreError> {
        let mut statement = self.conn.prepare_cached(
            "SELECT c.segment_id, c.previous_revision, r.text
             FROM llm_corrections c
             JOIN segment_revisions r
               ON r.segment_id = c.segment_id AND r.revision = c.previous_revision
             WHERE c.meeting_id = ?1 ORDER BY c.segment_id",
        )?;
        let rows = statement.query_map([meeting], |row| {
            Ok(Correction {
                segment_id: row.get(0)?,
                previous_revision: row.get(1)?,
                previous_text: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Keep LLM changes: one segment's, or all in the meeting when `segment` is None.
    pub fn keep_corrections(
        &mut self,
        meeting: i64,
        segment: Option<i64>,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM llm_corrections WHERE meeting_id = ?1 AND (?2 IS NULL OR segment_id = ?2)",
            params![meeting, segment],
        )?;
        Ok(())
    }

    /// Undo an LLM change: the text before it comes back as a new revision.
    pub fn revert_correction(&mut self, segment: i64) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        let previous: u32 = tx
            .query_row(
                "SELECT previous_revision FROM llm_corrections WHERE segment_id = ?1",
                [segment],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StoreError::NoCorrection(segment))?;
        let (text, source): (String, Source) = tx.query_row(
            "SELECT text, source FROM segment_revisions WHERE segment_id = ?1 AND revision = ?2",
            params![segment, previous],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        edit(&tx, segment, &text, source)?;
        tx.execute(
            "DELETE FROM llm_corrections WHERE segment_id = ?1",
            [segment],
        )?;
        tx.commit()?;
        Ok(())
    }
}
