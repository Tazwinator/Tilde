//! Command-level tests: the real `#[tauri::command]` functions, called through
//! Tauri's mock runtime against the bundled content DB and a fresh user DB.

use super::*;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::time::Duration;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

fn mock_app() -> tauri::App<MockRuntime> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let app_dir = std::env::temp_dir().join(format!(
        "tilde_cmd_test_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&app_dir).unwrap();
    let content = ContentDb::open(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/content.db"
    )));
    let user = db::open(&app_dir.join("tilde_user.db")).unwrap();
    let tts = tts::Tts::discover(&app_dir);
    mock_builder()
        .manage(AppState {
            content: Mutex::new(content),
            user: Mutex::new(user),
            tts,
            app_dir,
            sessions: Mutex::new(HashMap::new()),
            next_session: AtomicI64::new(1),
        })
        .build(mock_context(noop_assets()))
        .unwrap()
}

/// Runs `f` against a fresh mock app on its own thread, failing (rather than
/// hanging the test run) if it doesn't return — e.g. a mutex re-lock deadlock.
fn with_app<T: Send + 'static>(f: impl FnOnce(&tauri::App<MockRuntime>) -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let app = mock_app();
        let _ = tx.send(f(&app));
    });
    match rx.recv_timeout(Duration::from_secs(30)) {
        Ok(v) => v,
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("command did not return (deadlock?)"),
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("command panicked"),
    }
}

#[test]
fn commands_that_read_settings_do_not_deadlock() {
    with_app(|app| {
        let profile = profile_get(app.state());
        assert_eq!(profile.definition_lang, settings_get(app.state()).definition_lang);

        let start = session_start(app.state(), SessionKind::Standard);
        assert!(!start.rounds.is_empty());

        let hits = word_search(app.state(), "hablar".into(), 5);
        assert!(hits.iter().any(|h| h.lemma == "hablar"));
        assert!(content_credits(app.state()).contains("Wiktionary"));

        let hablar = hits.iter().find(|h| h.lemma == "hablar").unwrap();
        let detail = word_detail(app.state(), hablar.word_id).expect("word detail");
        assert_eq!(detail.word.word_id, hablar.word_id);
        let conj = detail.conjugations.expect("hablar has a conjugation table");
        assert!(conj.iter().any(|r| r.tense == "pretérito" && r.person == "yo" && r.form == "hablé"));
    });
}

/// Top-level keys of each `Round` variant as declared in `src/lib/contract.ts`,
/// keyed by the variant's `type` tag.
fn ts_round_keys() -> HashMap<String, Vec<String>> {
    let ts = include_str!("../../src/lib/contract.ts");
    let start = ts.find("export type Round =").expect("Round type in contract.ts");
    let end = start + ts[start..].find(";\n\n").expect("end of Round type");
    let mut out = HashMap::new();
    for variant in ts[start..end].split("| ({").skip(1) {
        let body = variant.split("})").next().unwrap();
        let mut keys: Vec<String> = body
            .split(';')
            .filter_map(|field| field.split(':').next())
            .map(|k| k.trim().trim_end_matches('?').to_string())
            .filter(|k| !k.is_empty())
            .collect();
        let tag = body.split('"').nth(1).expect("type tag").to_string();
        keys.sort();
        out.insert(tag, keys);
    }
    out
}

#[test]
fn round_wire_format_matches_contract_ts() {
    let word = tilde_core::types::WordCard {
        word_id: 1,
        lemma: "hablar".into(),
        pos: None,
        rank: 1,
        gloss_en: None,
        gloss_es: None,
        level: "A1".into(),
    };
    let samples = vec![
        Round::NewWord { id: 0, word, example_es: None, example_en: None, audio_base64: None },
        Round::Choice {
            id: 0,
            word_id: 1,
            prompt: String::new(),
            prompt_lang: "es".into(),
            options: vec![],
            answer_index: 0,
            audio_base64: None,
        },
        Round::Match { id: 0, pairs: vec![] },
        Round::Listen { id: 0, word_id: 1, audio_base64: None, options: vec![], answer_index: 0 },
        Round::ListenType {
            id: 0,
            word_id: 1,
            sentence_es: String::new(),
            audio_base64: None,
            answer: String::new(),
        },
        Round::Build {
            id: 0,
            word_id: 1,
            sentence_es: String::new(),
            sentence_en: String::new(),
            tiles: vec![],
            answer: String::new(),
        },
        Round::Cloze {
            id: 0,
            word_id: 1,
            sentence_es: String::new(),
            sentence_en: String::new(),
            options: vec![],
            answer_index: 0,
        },
        Round::Conjugation {
            id: 0,
            word_id: 1,
            verb: String::new(),
            tense: String::new(),
            person: String::new(),
            answer: String::new(),
            hint: String::new(),
        },
        Round::ReviewCard {
            id: 0,
            word_id: 1,
            es: String::new(),
            en: String::new(),
            example_es: None,
            example_en: None,
            audio_base64: None,
        },
    ];

    let mut rust: HashMap<String, Vec<String>> = HashMap::new();
    for round in &samples {
        let json = serde_json::to_value(round).unwrap();
        let obj = json.as_object().unwrap();
        let mut keys: Vec<String> = obj.keys().cloned().collect();
        keys.sort();
        rust.insert(obj["type"].as_str().unwrap().to_string(), keys);
    }
    assert_eq!(rust, ts_round_keys());
}

#[test]
fn srt_import_matches_words_from_content_db() {
    let srt = "1\n00:00:01,000 --> 00:00:03,000\nNo quiero ir a la escuela hoy.\n\n\
               2\n00:00:04,000 --> 00:00:06,000\n¿Dónde está la casa de tu madre?\n\n\
               3\n00:00:07,000 --> 00:00:09,000\nMi hermano vive en Madrid con su mujer.\n";
    let (report, listed) = with_app(move |app| {
        let report = srt_import(app.state(), "test.srt".into(), srt.into());
        (report, sentences_list(app.state()))
    });
    assert_eq!(report.sentences_found, 3);
    assert!(report.sentences_added > 0, "{report:?}");
    assert!(!report.top_words.is_empty());
    assert_eq!(listed.len() as i64, report.sentences_added);
}

fn count(app: &tauri::App<MockRuntime>, table: &str) -> i64 {
    let state = app.state::<AppState>();
    let conn = state.user.lock().unwrap();
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn backups_of_kind(app: &tauri::App<MockRuntime>, kind: &str) -> Vec<tilde_core::types::BackupInfo> {
    backup_folder(app.state())
        .backups
        .into_iter()
        .filter(|b| b.kind == kind)
        .collect()
}

fn zip_with(meta: &str, db: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    zip.start_file("meta.json", opts).unwrap();
    zip.write_all(meta.as_bytes()).unwrap();
    zip.start_file("user.db", opts).unwrap();
    zip.write_all(db).unwrap();
    zip.finish().unwrap().into_inner()
}

fn answer(app: &tauri::App<MockRuntime>, start: &SessionStart, index: usize, correct: bool) -> RoundFeedback {
    let result = RoundResult {
        round_index: index,
        correct,
        duration_ms: 1000,
        quality: None,
    };
    round_submit(app.state(), start.session_id, match_round_id(&start.rounds[index]), result)
}

/// Plays the first round of a fresh session and finishes it.
fn play_one_round(app: &tauri::App<MockRuntime>) {
    let start = session_start(app.state(), SessionKind::Standard);
    answer(app, &start, 0, true);
    session_finish(app.state(), start.session_id);
}

#[test]
fn xp_is_saved_as_it_is_earned_not_only_when_the_session_ends() {
    with_app(|app| {
        let start = session_start(app.state(), SessionKind::Standard);
        let earned: i64 = (0..start.rounds.len())
            .map(|i| i64::from(answer(app, &start, i, true).xp_gained))
            .sum();
        assert!(earned > 0);
        // walked away without finishing: the XP and the session still count
        let profile = profile_get(app.state());
        assert_eq!((profile.xp, profile.sessions_total), (earned, 1));

        let summary = session_finish(app.state(), start.session_id);
        assert_eq!(summary.xp, earned);
        let profile = profile_get(app.state());
        assert_eq!((profile.xp, profile.sessions_total), (earned, 1), "finishing counted it twice");
    });
}

#[test]
fn a_level_up_is_reported_when_lifetime_xp_crosses_a_level() {
    with_app(|app| {
        {
            let state = app.state::<AppState>();
            let conn = state.user.lock().unwrap();
            conn.execute("INSERT INTO events(ts, kind, xp) VALUES (0, 'session', 495)", [])
                .unwrap();
        }
        let start = session_start(app.state(), SessionKind::Standard);
        let level_ups: Vec<i32> = (0..start.rounds.len())
            .filter_map(|i| answer(app, &start, i, true).level_up)
            .map(|l| l.level)
            .collect();
        assert_eq!(level_ups, vec![1]);
        let summary = session_finish(app.state(), start.session_id);
        assert_eq!(summary.level_up.map(|l| l.level), Some(1));
    });
}

#[test]
fn a_finished_session_leaves_a_snapshot_that_restores_after_a_reset() {
    with_app(|app| {
        play_one_round(app);
        let autos = backups_of_kind(app, "auto");
        assert_eq!(autos.len(), 1, "{autos:?}");
        let cards = count(app, "cards");
        assert!(cards > 0);

        progress_reset(app.state()).unwrap();
        assert_eq!(count(app, "cards"), 0);
        assert_eq!(backups_of_kind(app, "before-reset").len(), 1);

        backup_restore(app.state(), autos[0].path.clone()).unwrap();
        assert_eq!(count(app, "cards"), cards);
        assert_eq!(backups_of_kind(app, "before-restore").len(), 1);
        // the restored connection is the live one: writes still land
        play_one_round(app);
        assert!(count(app, "round_log") >= 2);
    });
}

#[test]
fn a_bad_backup_is_refused_before_progress_is_touched() {
    with_app(|app| {
        let tener = word_search(app.state(), "tener".into(), 1)[0].word_id;
        word_mark_known(app.state(), tener);

        let newer = {
            let path = std::env::temp_dir().join(format!("tilde_newer_{}.db", std::process::id()));
            let _ = std::fs::remove_file(&path);
            let conn = db::open(&path).unwrap();
            conn.pragma_update(None, "user_version", db::SCHEMA_VERSION + 1).unwrap();
            conn.pragma_update(None, "journal_mode", "DELETE").unwrap();
            drop(conn);
            std::fs::read(&path).unwrap()
        };
        let not_tilde = {
            let path = std::env::temp_dir().join(format!("tilde_other_{}.db", std::process::id()));
            let _ = std::fs::remove_file(&path);
            rusqlite::Connection::open(&path)
                .unwrap()
                .execute_batch("CREATE TABLE notes(body TEXT); INSERT INTO notes VALUES ('hola');")
                .unwrap();
            std::fs::read(&path).unwrap()
        };
        let meta = r#"{"app":"tilde","format":1}"#;
        let bad: Vec<(&str, Vec<u8>)> = vec![
            ("garbage", b"definitely not a zip".to_vec()),
            ("other app", zip_with(r#"{"app":"other"}"#, &not_tilde)),
            ("garbage db", zip_with(meta, b"SQLite format 3\0 but then nonsense")),
            ("not tilde's tables", zip_with(meta, &not_tilde)),
            ("newer schema", zip_with(meta, &newer)),
        ];
        for (what, bytes) in bad {
            let err = backup_import(app.state(), bytes).expect_err(what);
            assert!(!err.is_empty());
            assert_eq!(count(app, "cards"), 1, "{what} touched progress");
        }
        assert!(backups_of_kind(app, "before-restore").is_empty());
        assert_eq!(profile_get(app.state()).words_learning, 1);
    });
}

#[test]
fn reset_clears_everything_at_once_including_mined_sentences() {
    with_app(|app| {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\nNo quiero ir a la escuela hoy.\n";
        srt_import(app.state(), "test.srt".into(), srt.into());
        play_one_round(app);
        assert!(count(app, "mined_words") > 0);

        progress_reset(app.state()).unwrap();
        for table in db::TABLES {
            assert_eq!(count(app, table), 0, "{table} not cleared");
        }
    });
}

#[test]
fn backups_go_to_the_chosen_folder_and_it_survives_a_restore() {
    with_app(|app| {
        let state = app.state::<AppState>();
        assert!(backup_folder(app.state()).is_default);
        assert!(backup_folder_set(app.state(), Some("relative/path".into())).is_err());

        let synced = state.app_dir.join("Sync").join("Tilde");
        let folder = backup_folder_set(app.state(), Some(synced.to_string_lossy().into_owned())).unwrap();
        assert!(!folder.is_default);
        assert_eq!(std::path::PathBuf::from(&folder.dir), synced);
        assert_eq!(folder.backups.len(), 1, "the new folder starts with a snapshot");

        let exported = backup_export(app.state()).unwrap();
        assert!(std::path::Path::new(&exported).starts_with(&synced));
        backup_restore(app.state(), exported).unwrap();
        assert_eq!(std::path::PathBuf::from(backup_folder(app.state()).dir), synced);

        assert!(backup_folder_set(app.state(), None).unwrap().is_default);
    });
}
