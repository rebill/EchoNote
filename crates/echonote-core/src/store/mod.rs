//! Meeting library in SQLite: meetings, recorded tracks, transcript segments with their
//! history, settings, and substring search. Markdown is an export, not the storage format.
//!
//! One writer connection (the store thread) and any number of read-only connections (UI
//! queries). WAL with `synchronous = NORMAL`: a committed batch survives an app crash. The
//! store holds rows only; recording files are the caller's to create and delete.

mod read;
mod retranscribe;
mod schema;
#[cfg(test)]
mod tests;
mod write;

use std::path::Path;
use std::time::Duration;

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use rusqlite::{Connection, OpenFlags};

pub use ai::{Correction, NewCorrection, NewSummary, QaTurn, SummaryRow, TemplateRow};
pub use read::SearchHit;
pub use retranscribe::Retranscribed;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(
        "the library was written by a newer EchoNote (schema {found}, this app knows {supported})"
    )]
    TooNew { found: u32, supported: u32 },
    #[error(
        "the library needs migrating (schema {found} of {supported}); open it for writing first"
    )]
    Outdated { found: u32, supported: u32 },
    #[error("no meeting {0}")]
    NoMeeting(i64),
    #[error("no segment {0}")]
    NoSegment(i64),
    #[error("segment {segment} has no revision {revision}")]
    NoRevision { segment: i64, revision: u32 },
    #[error(transparent)]
    Glossary(#[from] crate::glossary::GlossaryError),
    #[error("the glossary already has this term")]
    DuplicateTerm(String),
    #[error("the glossary is full ({} terms)", crate::glossary::MAX_ENTRIES)]
    GlossaryFull,
    #[error("no glossary entry {0}")]
    NoGlossaryEntry(i64),
    #[error("no template {0}")]
    NoTemplate(i64),
    #[error("segment {0} has no LLM correction to review")]
    NoCorrection(i64),
    #[error("a template name or its instructions are empty or too long")]
    InvalidTemplate,
    #[error("there are too many templates")]
    TooManyTemplates,
    #[error("the library is closed")]
    Closed,
}

/// A string enum stored as TEXT.
macro_rules! text_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(self.as_str().into())
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                match value.as_str()? {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(FromSqlError::InvalidType),
                }
            }
        }
    };
}

text_enum!(TrackKind { Mic => "mic", Sys => "sys" });
text_enum!(MeetingStatus {
    Recording => "recording",
    Finalized => "finalized",
    Recovered => "recovered",
});
text_enum!(
    /// Who wrote a segment's current text.
    Source { Asr => "asr", Llm => "llm", User => "user" }
);
text_enum!(
    /// Speaker bleed found in a mic segment: part of it was removed, or all of it repeats
    /// the system track and is hidden.
    Crosstalk { Trim => "trim", Duplicate => "duplicate" }
);
text_enum!(
    /// Notes written after the meeting, or the highlights kept while recording.
    SummaryKind { Notes => "notes", Highlights => "highlights" }
);

// After the macro: it is only visible below its definition.
mod ai;
#[derive(Clone, Debug, PartialEq)]
pub struct Meeting {
    pub id: i64,
    pub title: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub status: MeetingStatus,
    pub language: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: i64,
    pub meeting_id: i64,
    pub kind: TrackKind,
    /// Path relative to the meeting directory.
    pub file: String,
    pub sample_rate: u32,
    pub device_name: Option<String>,
    pub duration_ms: Option<i64>,
    pub transcribed_until_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub id: i64,
    pub meeting_id: i64,
    pub track: TrackKind,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub raw_text: Option<String>,
    pub model: String,
    pub source: Source,
    pub crosstalk: Option<Crosstalk>,
    pub revision: u32,
}

/// An earlier text of a segment.
#[derive(Clone, Debug, PartialEq)]
pub struct Revision {
    pub revision: u32,
    pub text: String,
    pub source: Source,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug)]
pub struct NewTrack {
    pub kind: TrackKind,
    pub file: String,
    pub sample_rate: u32,
    pub device_name: Option<String>,
}

/// A final from the live pipeline.
#[derive(Clone, Debug)]
pub struct NewSegment {
    pub track: TrackKind,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    /// ASR text before crosstalk trimming, when it differs.
    pub raw_text: Option<String>,
    pub model: String,
    pub crosstalk: Option<Crosstalk>,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open or create the library for writing, migrating it if needed. Before migrating an
    /// existing library it is copied to `<path>.v<old version>.bak`.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let mode: String =
            conn.pragma_update_and_check(None, "journal_mode", "wal", |row| row.get(0))?;
        debug_assert!(mode.eq_ignore_ascii_case("wal"), "journal_mode {mode}");
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut conn, path, schema::MIGRATIONS)?;
        Ok(Self { conn })
    }

    /// Open an existing, current library for queries only.
    pub fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(path, flags)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let found = user_version(&conn)?;
        let supported = schema::MIGRATIONS.len() as u32;
        if found > supported {
            return Err(StoreError::TooNew { found, supported });
        }
        if found < supported {
            return Err(StoreError::Outdated { found, supported });
        }
        Ok(Self { conn })
    }
}

fn user_version(conn: &Connection) -> rusqlite::Result<u32> {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
}

/// Apply the migrations after the stored `user_version`, in one transaction.
fn migrate(conn: &mut Connection, path: &Path, migrations: &[&str]) -> Result<(), StoreError> {
    let found = user_version(conn)?;
    let supported = migrations.len() as u32;
    if found > supported {
        return Err(StoreError::TooNew { found, supported });
    }
    if found == supported {
        return Ok(());
    }
    if found > 0 {
        backup(conn, path, found)?;
    }
    let tx = conn.transaction()?;
    for migration in &migrations[found as usize..] {
        tx.execute_batch(migration)?;
    }
    tx.pragma_update(None, "user_version", supported)?;
    tx.commit()?;
    Ok(())
}

/// A consistent copy of the database, WAL included.
fn backup(conn: &Connection, path: &Path, version: u32) -> Result<(), StoreError> {
    let mut target = path.as_os_str().to_owned();
    target.push(format!(".v{version}.bak"));
    let target = Path::new(&target);
    if target.exists() {
        std::fs::remove_file(target)?;
    }
    let Some(target) = target.to_str() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "library path is not valid UTF-8",
        )
        .into());
    };
    conn.execute("VACUUM INTO ?1", [target])?;
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}
