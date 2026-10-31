use std::path::PathBuf;

use super::*;
use crate::test_util::TempDir;

impl TempDir {
    fn db(&self) -> PathBuf {
        self.0.join("library.sqlite")
    }
}

fn final_(track: TrackKind, start_ms: i64, text: &str) -> NewSegment {
    NewSegment {
        track,
        start_ms,
        end_ms: start_ms + 1000,
        text: text.into(),
        raw_text: None,
        model: "sense-voice".into(),
        crosstalk: None,
    }
}

/// A store with one meeting and both tracks.
fn meeting(dir: &TempDir) -> (Store, i64) {
    let mut store = Store::open(&dir.db()).unwrap();
    let id = store.create_meeting("周会", 1_000, Some("zh")).unwrap();
    for kind in [TrackKind::Mic, TrackKind::Sys] {
        let track = NewTrack {
            kind,
            file: format!("{}.opus", kind.as_str()),
            sample_rate: 16_000,
            device_name: None,
        };
        store.add_track(id, &track).unwrap();
    }
    (store, id)
}

fn texts(hits: &[SearchHit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.text.as_str()).collect()
}

#[test]
fn a_new_library_is_migrated_and_reopens_unchanged() {
    let dir = TempDir::new();
    let store = Store::open(&dir.db()).unwrap();
    let mode: String = store
        .conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let foreign_keys: bool = store
        .conn
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .unwrap();
    assert!(foreign_keys);
    assert_eq!(
        user_version(&store.conn).unwrap() as usize,
        schema::MIGRATIONS.len()
    );
    drop(store);
    Store::open(&dir.db()).unwrap();
    // A brand-new library has nothing to back up.
    assert!(!dir.0.join("library.sqlite.v0.bak").exists());
}

#[test]
fn finals_and_progress_commit_together_and_unfinished_meetings_are_found() {
    let dir = TempDir::new();
    let (mut store, id) = meeting(&dir);
    store
        .commit_finals(
            id,
            &[
                final_(TrackKind::Sys, 2_000, "今天讨论预算"),
                final_(TrackKind::Mic, 500, "好的"),
            ],
            &[(TrackKind::Mic, 1_500), (TrackKind::Sys, 3_000)],
        )
        .unwrap();
    // Progress never moves backwards.
    store
        .commit_finals(id, &[], &[(TrackKind::Mic, 1_000)])
        .unwrap();
    let segments = store.segments(id).unwrap();
    assert_eq!(
        segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
        ["好的", "今天讨论预算"]
    );
    assert_eq!(segments[0].source, Source::Asr);
    assert_eq!(segments[0].revision, 1);
    let progress: Vec<(TrackKind, i64)> = store
        .tracks(id)
        .unwrap()
        .iter()
        .map(|track| (track.kind, track.transcribed_until_ms))
        .collect();
    assert_eq!(progress, [(TrackKind::Mic, 1_500), (TrackKind::Sys, 3_000)]);
    assert_eq!(store.unfinished_meetings().unwrap().len(), 1);

    store
        .finish_meeting(
            id,
            70_000,
            MeetingStatus::Finalized,
            &[(TrackKind::Mic, 65_000), (TrackKind::Sys, 64_000)],
        )
        .unwrap();
    let meeting = store.meeting(id).unwrap();
    assert_eq!(meeting.status, MeetingStatus::Finalized);
    assert_eq!(meeting.duration_ms, Some(65_000));
    assert!(store.unfinished_meetings().unwrap().is_empty());
}

#[test]
fn search_finds_chinese_substrings_and_english_words_of_any_length() {
    let dir = TempDir::new();
    let (mut store, id) = meeting(&dir);
    store
        .commit_finals(
            id,
            &[
                final_(TrackKind::Mic, 0, "我们下周发布新版本"),
                final_(TrackKind::Sys, 1_000, "The roadmap review is on Friday"),
                final_(TrackKind::Mic, 2_000, "转化率提升了 5%"),
                final_(TrackKind::Mic, 3_000, "file_name 要改"),
            ],
            &[],
        )
        .unwrap();
    assert_eq!(
        texts(&store.search("发布新版", None, 10).unwrap()),
        ["我们下周发布新版本"]
    );
    // One or two characters fall back to LIKE.
    assert_eq!(
        texts(&store.search("下周", None, 10).unwrap()),
        ["我们下周发布新版本"]
    );
    assert_eq!(store.search("版", None, 10).unwrap().len(), 1);
    assert_eq!(
        texts(&store.search("ROADMAP", None, 10).unwrap()),
        ["The roadmap review is on Friday"]
    );
    // LIKE wildcards and FTS syntax in the query are literal.
    assert_eq!(
        texts(&store.search("5%", None, 10).unwrap()),
        ["转化率提升了 5%"]
    );
    assert_eq!(
        texts(&store.search("e_n", None, 10).unwrap()),
        ["file_name 要改"]
    );
    assert!(store.search("_", None, 10).unwrap().len() == 1);
    assert!(store.search("\"review OR", None, 10).unwrap().is_empty());
    assert!(store.search("  ", None, 10).unwrap().is_empty());
    assert!(
        store
            .search("发布新版", Some(id + 1), 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn hidden_crosstalk_is_not_searchable_until_restored() {
    let dir = TempDir::new();
    let (mut store, id) = meeting(&dir);
    let mut duplicate = final_(TrackKind::Mic, 0, "季度目标已经完成");
    duplicate.crosstalk = Some(Crosstalk::Duplicate);
    let mut trimmed = final_(TrackKind::Mic, 2_000, "我同意");
    trimmed.raw_text = Some("季度目标我同意".into());
    trimmed.crosstalk = Some(Crosstalk::Trim);
    let ids = store
        .commit_finals(
            id,
            &[
                final_(TrackKind::Sys, 0, "季度目标已经完成"),
                duplicate,
                trimmed,
            ],
            &[],
        )
        .unwrap();
    let tracks = |hits: Vec<SearchHit>| hits.iter().map(|hit| hit.track).collect::<Vec<_>>();
    assert_eq!(
        tracks(store.search("季度目标", None, 10).unwrap()),
        [TrackKind::Sys]
    );
    assert_eq!(
        tracks(store.search("目标", None, 10).unwrap()),
        [TrackKind::Sys]
    );

    store.restore_crosstalk(ids[1]).unwrap();
    assert_eq!(store.search("季度目标", None, 10).unwrap().len(), 2);
    store.restore_crosstalk(ids[2]).unwrap();
    let restored = store.segment(ids[2]).unwrap();
    assert_eq!(restored.text, "季度目标我同意");
    assert_eq!(restored.crosstalk, None);
    assert_eq!(restored.source, Source::User);
    assert_eq!(store.search("季度目标", None, 10).unwrap().len(), 3);
    assert_eq!(store.revisions(ids[2]).unwrap()[0].text, "我同意");
}

#[test]
fn edits_keep_history_and_replace_the_indexed_text() {
    let dir = TempDir::new();
    let (mut store, id) = meeting(&dir);
    let segment = store
        .commit_finals(id, &[final_(TrackKind::Mic, 0, "我们用瑞克特框架")], &[])
        .unwrap()[0];
    store
        .edit_segment(segment, "我们用 React 框架", Source::Llm)
        .unwrap();
    store
        .edit_segment(segment, "我们用 React 前端框架", Source::User)
        .unwrap();
    assert!(store.search("瑞克特", None, 10).unwrap().is_empty());
    assert_eq!(store.search("react", None, 10).unwrap().len(), 1);
    let current = store.segment(segment).unwrap();
    assert_eq!((current.revision, current.source), (3, Source::User));
    let history: Vec<(u32, String, Source)> = store
        .revisions(segment)
        .unwrap()
        .into_iter()
        .map(|revision| (revision.revision, revision.text, revision.source))
        .collect();
    assert_eq!(
        history,
        [
            (1, "我们用瑞克特框架".into(), Source::Asr),
            (2, "我们用 React 框架".into(), Source::Llm)
        ]
    );
    assert!(matches!(
        store.edit_segment(segment + 99, "x", Source::User),
        Err(StoreError::NoSegment(_))
    ));
}

#[test]
fn restoring_a_revision_is_itself_an_edit() {
    let dir = TempDir::new();
    let (mut store, id) = meeting(&dir);
    let segment = store
        .commit_finals(id, &[final_(TrackKind::Mic, 0, "我们用瑞克特框架")], &[])
        .unwrap()[0];
    store
        .edit_segment(segment, "我们用 React 框架", Source::User)
        .unwrap();
    store.restore_revision(segment, 1).unwrap();
    let current = store.segment(segment).unwrap();
    assert_eq!(
        (current.text.as_str(), current.source, current.revision),
        ("我们用瑞克特框架", Source::Asr, 3)
    );
    assert_eq!(store.search("瑞克特", None, 10).unwrap().len(), 1);
    // The restore can be undone, and restoring the current revision changes nothing.
    store.restore_revision(segment, 2).unwrap();
    assert_eq!(store.segment(segment).unwrap().text, "我们用 React 框架");
    store.restore_revision(segment, 4).unwrap();
    assert_eq!(store.segment(segment).unwrap().revision, 4);
    assert!(matches!(
        store.restore_revision(segment, 9),
        Err(StoreError::NoRevision { revision: 9, .. })
    ));
}

#[test]
fn glossary_entries_are_unique_per_term_and_searchable_by_any_spelling() {
    let dir = TempDir::new();
    let (mut store, id) = meeting(&dir);
    let aliases = |list: &[&str]| {
        list.iter()
            .map(|alias| (*alias).to_owned())
            .collect::<Vec<_>>()
    };
    let k8s = store
        .save_glossary_entry(None, " Kubernetes ", &aliases(&["库伯内特斯", "", "K8"]))
        .unwrap();
    let feishu = store
        .save_glossary_entry(None, "飞书", &aliases(&["飞鼠"]))
        .unwrap();
    assert!(matches!(
        store.save_glossary_entry(None, "kubernetes", &[]),
        Err(StoreError::DuplicateTerm(term)) if term == "kubernetes"
    ));
    assert!(matches!(
        store.save_glossary_entry(Some(feishu), "Kubernetes", &[]),
        Err(StoreError::DuplicateTerm(_))
    ));
    assert!(matches!(
        store.save_glossary_entry(Some(999), "x", &[]),
        Err(StoreError::NoGlossaryEntry(999))
    ));
    assert!(matches!(
        store.save_glossary_entry(None, " ", &[]),
        Err(StoreError::Glossary(_))
    ));
    store
        .save_glossary_entry(Some(feishu), "飞书", &aliases(&["飞鼠", "非书"]))
        .unwrap();
    let entries = store.glossary().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        (
            entries[0].id,
            entries[0].term.as_str(),
            entries[0].aliases.clone()
        ),
        (k8s, "Kubernetes", aliases(&["库伯内特斯", "K8"]))
    );
    assert_eq!(entries[1].aliases, aliases(&["飞鼠", "非书"]));

    store
        .commit_finals(
            id,
            &[
                final_(TrackKind::Mic, 0, "库伯内特斯集群要升级"),
                final_(TrackKind::Sys, 1_000, "发到非书群里"),
                final_(TrackKind::Mic, 2_000, "别的事情"),
            ],
            &[],
        )
        .unwrap();
    let glossary = crate::glossary::Glossary::new(&entries);
    let hits = store
        .search_any(&glossary.search_variants("kubernetes集群"), None, 10)
        .unwrap();
    assert_eq!(texts(&hits), ["库伯内特斯集群要升级"]);
    // Short spellings fall back to LIKE and still combine with the index.
    let hits = store
        .search_any(&glossary.search_variants("飞书"), None, 10)
        .unwrap();
    assert_eq!(texts(&hits), ["发到非书群里"]);
    store.delete_glossary_entry(k8s).unwrap();
    assert!(matches!(
        store.delete_glossary_entry(k8s),
        Err(StoreError::NoGlossaryEntry(_))
    ));
    assert_eq!(store.glossary().unwrap().len(), 1);
}

#[test]
fn deleting_a_meeting_removes_its_rows_and_index_entries() {
    let dir = TempDir::new();
    let (mut store, id) = meeting(&dir);
    let other = store.create_meeting("复盘", 5_000, None).unwrap();
    let segment = store
        .commit_finals(id, &[final_(TrackKind::Mic, 0, "删除这个会议")], &[])
        .unwrap()[0];
    store
        .edit_segment(segment, "删除这个会议吧", Source::User)
        .unwrap();
    store
        .commit_finals(other, &[final_(TrackKind::Mic, 0, "保留这个会议")], &[])
        .unwrap();
    store.delete_meeting(id).unwrap();
    let count = |sql: &str| -> i64 { store.conn.query_row(sql, [], |row| row.get(0)).unwrap() };
    assert_eq!(count("SELECT count(*) FROM tracks"), 0);
    assert_eq!(count("SELECT count(*) FROM segments"), 1);
    assert_eq!(count("SELECT count(*) FROM segment_revisions"), 0);
    assert_eq!(count("SELECT count(*) FROM segments_fts"), 1);
    assert!(store.search("删除这个", None, 10).unwrap().is_empty());
    assert_eq!(store.meetings().unwrap().len(), 1);
    assert!(matches!(
        store.delete_meeting(id),
        Err(StoreError::NoMeeting(_))
    ));
}

#[test]
fn migrating_an_existing_library_backs_it_up_first() {
    let dir = TempDir::new();
    let (store, id) = meeting(&dir);
    drop(store);
    let mut conn = Connection::open(dir.db()).unwrap();
    let known = schema::MIGRATIONS.len() as u32;
    let mut next = schema::MIGRATIONS.to_vec();
    next.push("CREATE TABLE extra (id INTEGER PRIMARY KEY);");
    migrate(&mut conn, &dir.db(), &next).unwrap();
    assert_eq!(user_version(&conn).unwrap(), known + 1);
    let backup = Connection::open(dir.0.join(format!("library.sqlite.v{known}.bak"))).unwrap();
    assert_eq!(user_version(&backup).unwrap(), known);
    let title: String = backup
        .query_row("SELECT title FROM meetings WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(title, "周会");
    // This app does not know the extra migration.
    drop(conn);
    assert!(matches!(
        Store::open(&dir.db()),
        Err(StoreError::TooNew { found, supported }) if found == known + 1 && supported == known
    ));
}

#[test]
fn a_reader_sees_committed_rows_and_cannot_write() {
    let dir = TempDir::new();
    let (mut store, id) = meeting(&dir);
    let mut reader = Store::open_read_only(&dir.db()).unwrap();
    store
        .commit_finals(id, &[final_(TrackKind::Sys, 0, "读写分离测试")], &[])
        .unwrap();
    assert_eq!(reader.search("读写分离", None, 10).unwrap().len(), 1);
    assert!(reader.rename_meeting(id, "改名").is_err());
    store.set_setting("model", "sense-voice").unwrap();
    store.set_setting("model", "qwen3-asr").unwrap();
    assert_eq!(
        reader.setting("model").unwrap().as_deref(),
        Some("qwen3-asr")
    );
    assert_eq!(reader.setting("missing").unwrap(), None);
}
