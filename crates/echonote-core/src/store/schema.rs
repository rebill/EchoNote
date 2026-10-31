//! Schema migrations. Entry `n` takes the database from `user_version` n to n + 1. Never edit
//! a released entry; append a new one.

pub const MIGRATIONS: &[&str] = &[V1, V2, V3, V4];

/// Meetings, their recorded tracks and transcript segments, segment history, settings, and the
/// search index. The index is contentless and holds only visible text: segments hidden as
/// crosstalk duplicates are not indexed, and edits replace the indexed text.
const V1: &str = r#"
CREATE TABLE meetings (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    started_at_ms INTEGER NOT NULL,
    ended_at_ms INTEGER,
    duration_ms INTEGER,
    status TEXT NOT NULL CHECK (status IN ('recording', 'finalized', 'recovered')),
    language TEXT
);
CREATE INDEX meetings_started ON meetings (started_at_ms);
CREATE INDEX meetings_status ON meetings (status);

CREATE TABLE tracks (
    id INTEGER PRIMARY KEY,
    meeting_id INTEGER NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('mic', 'sys')),
    file TEXT NOT NULL,
    sample_rate INTEGER NOT NULL,
    device_name TEXT,
    duration_ms INTEGER,
    -- Audio before this point has final segments in the database (crash recovery).
    transcribed_until_ms INTEGER NOT NULL DEFAULT 0,
    UNIQUE (meeting_id, kind)
);

CREATE TABLE segments (
    id INTEGER PRIMARY KEY,
    meeting_id INTEGER NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    track TEXT NOT NULL CHECK (track IN ('mic', 'sys')),
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL CHECK (end_ms >= start_ms),
    text TEXT NOT NULL,
    -- ASR output before crosstalk trimming or edits; NULL when equal to the first text.
    raw_text TEXT,
    model TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('asr', 'llm', 'user')),
    crosstalk TEXT CHECK (crosstalk IN ('trim', 'duplicate')),
    revision INTEGER NOT NULL DEFAULT 1,
    updated_at_ms INTEGER NOT NULL
);
CREATE INDEX segments_meeting ON segments (meeting_id, start_ms);

-- Earlier texts of a segment; the current one lives in `segments`.
CREATE TABLE segment_revisions (
    segment_id INTEGER NOT NULL REFERENCES segments (id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    text TEXT NOT NULL,
    source TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (segment_id, revision)
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE VIRTUAL TABLE segments_fts USING fts5 (
    text,
    content = '',
    contentless_delete = 1,
    tokenize = 'trigram'
);

CREATE TRIGGER segments_fts_insert AFTER INSERT ON segments
WHEN new.crosstalk IS NOT 'duplicate'
BEGIN
    INSERT INTO segments_fts (rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER segments_fts_update AFTER UPDATE OF text, crosstalk ON segments
BEGIN
    DELETE FROM segments_fts WHERE rowid = old.id;
    INSERT INTO segments_fts (rowid, text)
    SELECT new.id, new.text WHERE new.crosstalk IS NOT 'duplicate';
END;

CREATE TRIGGER segments_fts_delete AFTER DELETE ON segments
BEGIN
    DELETE FROM segments_fts WHERE rowid = old.id;
END;
"#;

/// The user's glossary (`crate::glossary`): a term and the ways it gets misrecognized, one
/// alias per line. Terms are unique regardless of ASCII case.
const V2: &str = r#"
CREATE TABLE glossary (
    id INTEGER PRIMARY KEY,
    term TEXT NOT NULL UNIQUE COLLATE NOCASE,
    aliases TEXT NOT NULL DEFAULT '',
    updated_at_ms INTEGER NOT NULL
);
"#;

/// AI results. `summaries` keeps every generated set of notes (the latest is shown) and the
/// live highlights (one row per meeting, replaced). `qa` holds question/answer turns.
/// `templates` are the user's own summary templates. `llm_corrections` lists segments the
/// LLM changed that the user has not reviewed, with the revision it replaced.
const V3: &str = r#"
CREATE TABLE summaries (
    id INTEGER PRIMARY KEY,
    meeting_id INTEGER NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('notes', 'highlights')),
    template TEXT NOT NULL,
    provider TEXT NOT NULL,
    title TEXT,
    content TEXT NOT NULL,
    truncated INTEGER NOT NULL DEFAULT 0,
    created_at_ms INTEGER NOT NULL
);
CREATE INDEX summaries_meeting ON summaries (meeting_id, kind, created_at_ms);

CREATE TABLE qa (
    id INTEGER PRIMARY KEY,
    meeting_id INTEGER NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    question TEXT NOT NULL,
    answer TEXT NOT NULL,
    provider TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);
CREATE INDEX qa_meeting ON qa (meeting_id, id);

CREATE TABLE templates (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    instructions TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE llm_corrections (
    segment_id INTEGER PRIMARY KEY REFERENCES segments (id) ON DELETE CASCADE,
    meeting_id INTEGER NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    previous_revision INTEGER NOT NULL
);
CREATE INDEX llm_corrections_meeting ON llm_corrections (meeting_id);
"#;

/// Segments transcribed again with the high-accuracy model: the revision and model they had
/// before the first pass (what "undo" goes back to), and the revision that pass produced (a
/// later edit makes the row stale, and undo leaves that segment alone).
const V4: &str = r#"
CREATE TABLE retranscriptions (
    segment_id INTEGER PRIMARY KEY REFERENCES segments (id) ON DELETE CASCADE,
    meeting_id INTEGER NOT NULL REFERENCES meetings (id) ON DELETE CASCADE,
    previous_revision INTEGER NOT NULL,
    previous_model TEXT NOT NULL,
    revision INTEGER NOT NULL
);
CREATE INDEX retranscriptions_meeting ON retranscriptions (meeting_id);
"#;
