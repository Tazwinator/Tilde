//! Command-level tests: the real `#[tauri::command]` functions, called through
//! Tauri's mock runtime against the bundled content DB and a fresh user DB.

use super::*;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::time::Duration;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

fn mock_app() -> tauri::App<MockRuntime> {
    mock_app_speaking(tts::Tts::off)
}

fn mock_app_speaking(tts: impl FnOnce(&Path) -> tts::Tts) -> tauri::App<MockRuntime> {
    let app_dir = fresh_app_dir();
    let user = db::open(&user_db_path(&app_dir)).unwrap();
    let tts = tts(&app_dir);
    mock_app_over(app_dir, user, tts)
}

/// A new, empty data directory.
fn fresh_app_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let app_dir = std::env::temp_dir().join(format!(
        "tilde_cmd_test_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&app_dir);
    std::fs::create_dir_all(&app_dir).unwrap();
    app_dir
}

/// A mock app over a data directory and progress connection the test made.
fn mock_app_over(app_dir: PathBuf, user: Connection, tts: tts::Tts) -> tauri::App<MockRuntime> {
    let content = ContentDb::open(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/content.db"
    )))
    .unwrap();
    mock_builder()
        .manage(AppState::new(content, user, tts, app_dir))
        .build(mock_context(noop_assets()))
        .unwrap()
}

/// Runs `f` against a fresh mock app on its own thread, failing (rather than
/// hanging the test run) if it doesn't return — e.g. a mutex re-lock deadlock.
fn with_app<T: Send + 'static>(f: impl FnOnce(&tauri::App<MockRuntime>) -> T + Send + 'static) -> T {
    with(mock_app, f)
}

fn with<T: Send + 'static>(
    app: impl FnOnce() -> tauri::App<MockRuntime> + Send + 'static,
    f: impl FnOnce(&tauri::App<MockRuntime>) -> T + Send + 'static,
) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let app = app();
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
        assert!(content_credits(app.state()).contains("Wikcionario"));

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
        Round::Listen { id: 0, word_id: 1, es: String::new(), audio_base64: None, options: vec![], answer_index: 0 },
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
        let report = srt_import(app.state(), "test.srt".into(), srt.into()).unwrap();
        (report, sentences_list(app.state(), None, None).sentences)
    });
    assert_eq!(report.sentences_found, 3);
    assert!(report.sentences_added > 0, "{report:?}");
    assert!(!report.top_words.is_empty());
    assert_eq!(listed.len() as i64, report.sentences_added);

    // the same file again adds nothing new
    let again = with_app(move |app| {
        srt_import(app.state(), "test.srt".into(), srt.into()).unwrap();
        let second = srt_import(app.state(), "test.srt".into(), srt.into()).unwrap();
        (second.sentences_added, sentences_list(app.state(), None, None).sentences.len())
    });
    assert_eq!(again, (0, listed.len()));
}

#[test]
fn a_sentence_from_another_file_is_not_mined_twice() {
    let first = "1\n00:00:01,000 --> 00:00:03,000\nNo quiero ir a la escuela hoy.\n\n\
                 2\n00:00:04,000 --> 00:00:06,000\nMi hermano vive en Madrid con su mujer.\n";
    // the same lines as another subtitler wrote them, and one new one
    let second = "1\n00:00:01,000 --> 00:00:03,000\n- No quiero ir a la escuela, hoy...\n\n\
                  2\n00:00:04,000 --> 00:00:06,000\n¡MI HERMANO vive en Madrid con su mujer!\n\n\
                  3\n00:00:07,000 --> 00:00:09,000\n¿Dónde está la casa de tu madre?\n";
    let (added, listed) = with_app(move |app| {
        srt_import(app.state(), "uno.srt".into(), first.into()).unwrap();
        let report = srt_import(app.state(), "dos.srt".into(), second.into()).unwrap();
        (report.sentences_added, sentences_list(app.state(), None, None).sentences)
    });
    assert_eq!(added, 1, "{listed:?}");
    assert_eq!(listed.len(), 3);
    assert_eq!(listed[0].es, "¿Dónde está la casa de tu madre?");
}

/// Imports `text` as a subtitle file called `title`.
fn mine(app: &tauri::App<MockRuntime>, title: &str, text: &str) -> ImportReport {
    srt_import(app.state(), title.into(), text.into()).unwrap()
}

/// Whether a mined sentence is linked to the word with exactly this lemma.
fn mined_word(app: &tauri::App<MockRuntime>, lemma: &str) -> bool {
    // the search also looks inside glosses and longer lemmas: "té" finds
    // "saltar" (to sauté) first
    let word_id = word_search(app.state(), lemma.into(), 20)
        .into_iter()
        .find(|w| w.lemma == lemma)
        .unwrap_or_else(|| panic!("no word {lemma}"))
        .word_id;
    let state = app.state::<AppState>();
    let conn = state.user.lock().unwrap();
    conn.query_row("SELECT EXISTS(SELECT 1 FROM mined_words WHERE word_id = ?1)", [word_id], |r| r.get(0))
        .unwrap()
}

#[test]
fn subtitle_mining_credits_estar_saber_and_te() {
    let srt = "1\n00:00:01,000 --> 00:00:04,000\nMi madre está en casa y no sé si quiere té.\n";
    with_app(move |app| {
        mine(app, "test.srt", srt);
        // "está", "sé" and "té" are not the stopwords "esta", "se" and "te"
        for lemma in ["estar", "saber", "té"] {
            assert!(mined_word(app, lemma), "{lemma} not linked");
        }
    });
}

#[test]
fn an_ellipsis_split_sentence_is_mined_whole() {
    let srt = "1\n00:00:01,000 --> 00:00:03,000\nSi me hubieras dicho la verdad...\n\n\
               2\n00:00:03,500 --> 00:00:05,000\n...no estaríamos aquí ahora.\n";
    let listed = with_app(move |app| {
        mine(app, "test.srt", srt);
        sentences_list(app.state(), None, None).sentences
    });
    let texts: Vec<&str> = listed.iter().map(|s| s.es.as_str()).collect();
    assert_eq!(texts, ["Si me hubieras dicho la verdad... no estaríamos aquí ahora."]);
}

#[test]
fn sentences_that_differ_in_a_number_are_both_mined() {
    let first = "1\n00:00:01,000 --> 00:00:03,000\nTengo 30 años y dos hijos.\n";
    let second = "1\n00:00:01,000 --> 00:00:03,000\nTengo 40 años y dos hijos.\n";
    let (added, again) = with_app(move |app| {
        mine(app, "uno.srt", first);
        let added = mine(app, "dos.srt", second).sentences_added;
        (added, mine(app, "dos.srt", second).sentences_added)
    });
    assert_eq!((added, again), (1, 0));
}

#[test]
fn lines_with_nothing_to_learn_are_not_mined() {
    let srt = "1\n00:00:01,000 --> 00:00:03,000\n1, 2, 3.\n\n\
               2\n00:00:04,000 --> 00:00:06,000\nSí, sí, ya voy.\n";
    let report = with_app(move |app| mine(app, "test.srt", srt));
    assert_eq!((report.sentences_found, report.sentences_added), (2, 0), "{report:?}");
}

#[test]
fn a_decomposed_subtitle_file_is_stored_composed() {
    // as macOS tools write it: "a" + U+0301 for "á"
    let srt = "1\n00:00:01,000 --> 00:00:03,000\nMi madre esta\u{301} en casa.\n";
    with_app(move |app| {
        mine(app, "test.srt", srt);
        assert_eq!(sentences_list(app.state(), None, None).sentences[0].es, "Mi madre est\u{e1} en casa.");
        assert!(mined_word(app, "estar"), "estar not linked");

        // a sentence an older version stored decomposed is still recognised
        app.state::<AppState>()
            .user
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO mined(es, source, created_at) VALUES ('Man\u{303}ana no quiero ir a la escuela.', 'vieja.srt', 0)",
                [],
            )
            .unwrap();
        assert_eq!(mine(app, "otra.srt", "Ma\u{f1}ana no quiero ir a la escuela.").sentences_added, 0);
    });
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
    try_answer(app, start, index, correct).unwrap()
}

fn try_answer(
    app: &tauri::App<MockRuntime>,
    start: &SessionStart,
    index: usize,
    correct: bool,
) -> Result<RoundFeedback, String> {
    submit(app, start, index, correct, 1000, None)
}

fn submit(
    app: &tauri::App<MockRuntime>,
    start: &SessionStart,
    index: usize,
    correct: bool,
    duration_ms: i64,
    quality: Option<i32>,
) -> Result<RoundFeedback, String> {
    let result = RoundResult { round_index: index, correct, duration_ms, quality, missed_word_ids: vec![] };
    round_submit(app.state(), start.session_id, match_round_id(&start.rounds[index]), result)
}

/// The first round that asks something (not an intro card).
fn first_quiz(start: &SessionStart) -> usize {
    start.rounds.iter().position(|r| !matches!(r, Round::NewWord { .. })).expect("a round that isn't an intro")
}

/// The words a round asks about; none for an intro card, which only shows one.
fn quizzed(round: &Round) -> Vec<i64> {
    match round {
        Round::NewWord { .. } => vec![],
        Round::Match { pairs, .. } => pairs.iter().map(|p| p.word_id).collect(),
        Round::Choice { word_id, .. }
        | Round::Listen { word_id, .. }
        | Round::ListenType { word_id, .. }
        | Round::Build { word_id, .. }
        | Round::Cloze { word_id, .. }
        | Round::Conjugation { word_id, .. }
        | Round::ReviewCard { word_id, .. } => vec![*word_id],
    }
}

/// Runs `f` on the app's own progress connection.
fn user<T>(app: &tauri::App<MockRuntime>, f: impl FnOnce(&Connection) -> T) -> T {
    f(&app.state::<AppState>().user.lock().unwrap())
}

/// Every word that can be taught, commonest first.
fn teachable(app: &tauri::App<MockRuntime>) -> Vec<tilde_core::types::WordCard> {
    app.state::<AppState>().content.lock().unwrap().new_candidates(usize::MAX, &HashSet::new())
}

/// (stability, state) of the word's card, if it has one.
fn card(app: &tauri::App<MockRuntime>, word_id: i64) -> Option<(f64, i64)> {
    user(app, |conn| {
        conn.query_row("SELECT stability, state FROM cards WHERE word_id = ?1", [word_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .ok()
    })
}

fn earned(app: &tauri::App<MockRuntime>) -> Vec<String> {
    badges_list(app.state()).into_iter().filter(|b| b.earned_at.is_some()).map(|b| b.id).collect()
}

fn ids(badges: &[Badge]) -> Vec<&str> {
    badges.iter().map(|b| b.id.as_str()).collect()
}

/// Ids no word has: what a card is left with when its word leaves the
/// dictionary in a later build.
fn orphan_ids(app: &tauri::App<MockRuntime>, n: i64) -> Vec<i64> {
    let state = app.state::<AppState>();
    let highest = state.content.lock().unwrap().word_ids().iter().copied().max().unwrap();
    (0..n).map(|k| highest + 1000 + k).collect()
}

/// Cards for `word_ids`, known for a good while (`state` 2) or still being
/// learned (1), and due since `due_ts`.
fn insert_cards(app: &tauri::App<MockRuntime>, word_ids: &[i64], state: i64, due_ts: i64) {
    user(app, |conn| {
        for id in word_ids {
            conn.execute(
                "INSERT INTO cards(word_id, stability, difficulty, due_ts, last_review_ts, reps, lapses, state)
                 VALUES (?1, 100.0, 5.0, ?2, 0, 3, 0, ?3)",
                [*id, due_ts, state],
            )
            .unwrap();
        }
    })
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
        word_mark_known(app.state(), tener).unwrap();

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
        srt_import(app.state(), "test.srt".into(), srt.into()).unwrap();
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

#[test]
fn a_stale_session_or_a_repeat_submit_grades_nothing() {
    with_app(|app| {
        let start = session_start(app.state(), SessionKind::Standard);
        let ghost = SessionStart { session_id: 9_999, ..start.clone() };
        let feedback = answer(app, &ghost, 0, true);
        assert_eq!(feedback.xp_gained, 0);

        let i = start.rounds.iter().position(|r| !matches!(r, Round::NewWord { .. })).unwrap();
        assert!(answer(app, &start, i, true).xp_gained > 0);
        assert_eq!(answer(app, &start, i, true).xp_gained, 0, "second submit was graded");
        assert_eq!(count(app, "round_log"), 1);
        assert_eq!(count(app, "reviews"), 1);
        // the sessions mutex wasn't poisoned by a panic
        session_finish(app.state(), start.session_id);
    });
}

#[test]
fn a_match_round_grades_each_pair_on_its_own() {
    with_app(|app| {
        let start = session_start(app.state(), SessionKind::Standard);
        let (index, pairs) = start
            .rounds
            .iter()
            .enumerate()
            .find_map(|(i, r)| match r {
                Round::Match { pairs, .. } => Some((i, pairs.clone())),
                _ => None,
            })
            .expect("a standard session has a match round");
        let missed = pairs[0].word_id;
        let result = RoundResult {
            round_index: index,
            correct: false,
            duration_ms: 5000,
            quality: None,
            missed_word_ids: vec![missed],
        };
        round_submit(app.state(), start.session_id, match_round_id(&start.rounds[index]), result).unwrap();

        let state = app.state::<AppState>();
        let conn = state.user.lock().unwrap();
        for p in &pairs {
            let lapses: i64 = conn
                .query_row("SELECT lapses FROM cards WHERE word_id = ?1", [p.word_id], |r| r.get(0))
                .unwrap();
            assert_eq!(lapses, i64::from(p.word_id == missed), "{}", p.es);
        }
    });
}

fn new_word_ids(start: &SessionStart) -> Vec<i64> {
    start
        .rounds
        .iter()
        .filter_map(|r| match r {
            Round::NewWord { word, .. } => Some(word.word_id),
            _ => None,
        })
        .collect()
}

#[test]
fn starting_a_session_creates_no_cards_until_rounds_are_answered() {
    with_app(|app| {
        let start = session_start(app.state(), SessionKind::Standard);
        assert!(!new_word_ids(&start).is_empty());
        assert_eq!(count(app, "cards"), 0, "an unplayed session left cards behind");
        answer(app, &start, 0, true);
        assert!(count(app, "cards") >= 1);
    });
}

#[test]
fn new_words_keep_coming_past_every_word_already_in_the_deck() {
    with_app(|app| {
        // the 300 most frequent teachable words are already known
        let first = {
            let state = app.state::<AppState>();
            let content = state.content.lock().unwrap();
            content.new_candidates(300, &HashSet::new())
        };
        for w in &first {
            word_mark_known(app.state(), w.word_id).unwrap();
        }
        let known: HashSet<i64> = first.iter().map(|w| w.word_id).collect();

        let mut seen = HashSet::new();
        for _ in 0..3 {
            let start = session_start(app.state(), SessionKind::Standard);
            let ids = new_word_ids(&start);
            assert!(ids.len() >= 2, "ran out of new words");
            for id in ids {
                assert!(!known.contains(&id), "reintroduced a known word");
                assert!(seen.insert(id), "same new word in two sessions");
            }
            for i in 0..start.rounds.len() {
                answer(app, &start, i, true);
            }
            session_finish(app.state(), start.session_id);
        }
    });
}

#[test]
fn stats_use_real_durations_and_run_oldest_to_newest() {
    with_app(|app| {
        {
            let state = app.state::<AppState>();
            let conn = state.user.lock().unwrap();
            let now = deck::now() as i64;
            conn.execute_batch(&format!(
                "INSERT INTO events(ts, kind, xp, duration_s) VALUES ({}, 'session', 50, 600);
                 INSERT INTO events(ts, kind, xp, duration_s) VALUES ({}, 'session', 100, 1200);
                 INSERT INTO round_log(ts, session_id, round_type, correct) VALUES
                   ({now}, 1, 'new_word', 1), ({now}, 1, 'new_word', 1), ({now}, 1, 'new_word', 1),
                   ({now}, 1, 'choice', 1), ({now}, 1, 'choice', 0);",
                now - 86_400,
                now - 15 * 86_400,
            ))
            .unwrap();
        }
        let stats = stats_get(app.state());
        let labels: Vec<&str> = stats.minutes_per_week.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["-7 sem", "-6 sem", "-5 sem", "-4 sem", "-3 sem", "-2 sem", "-1 sem", "ahora"]);
        let minutes: Vec<f64> = stats.minutes_per_week.iter().map(|p| p.value).collect();
        assert_eq!(minutes, [0.0, 0.0, 0.0, 0.0, 0.0, 20.0, 0.0, 10.0]);
        assert_eq!(stats.xp_per_week.last().map(|p| p.value), Some(50.0));
        // intros aren't answers: they count in neither accuracy nor the game mix
        assert_eq!(stats.accuracy_all_time, Some(0.5));
        assert!(stats.game_mix.iter().all(|g| g.label != "new_word"));
    });
}

#[test]
fn every_logged_round_type_has_a_stats_label() {
    let page = include_str!("../../src/routes/stats/+page.svelte");
    let start = page.find("const GAME_LABELS").expect("GAME_LABELS in stats page");
    let body = &page[start..start + page[start..].find("};").unwrap()];
    let labelled: HashSet<String> = body
        .lines()
        .filter_map(|l| l.trim().split_once(':'))
        .map(|(k, _)| k.trim().to_string())
        .collect();
    for tag in ts_round_keys().into_keys().filter(|t| t != "new_word") {
        assert!(labelled.contains(&tag), "no stats label for round type {tag}");
    }
}

#[cfg(unix)]
#[test]
fn starting_a_session_does_not_wait_for_speech() {
    use std::os::unix::fs::PermissionsExt;
    // a voice that takes half a second per phrase, like Piper on a cold start
    let slow_voice = |app_dir: &std::path::Path| {
        let script = app_dir.join("slow-espeak");
        std::fs::write(
            &script,
            "#!/bin/sh\nout=\"\"\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = \"-w\" ]; then out=\"$2\"; shift; fi\n  shift\ndone\nsleep 0.5\ncat > \"$out\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        tts::Tts::with_espeak(app_dir, script)
    };
    with(
        move || mock_app_speaking(slow_voice),
        |app| {
            let started = std::time::Instant::now();
            let start = session_start(app.state(), SessionKind::Standard);
            let took = started.elapsed();
            // 18 rounds at half a second each would be 9 s and more
            assert!(took < Duration::from_secs(2), "session_start took {took:?}");

            // ...and the first round's audio turns up in the background
            let Round::NewWord { word, .. } = &start.rounds[0] else { panic!("first round is an intro") };
            let state = app.state::<AppState>();
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while state.tts.cached(&word.lemma).is_none() {
                assert!(std::time::Instant::now() < deadline, "audio for {} never arrived", word.lemma);
                std::thread::sleep(Duration::from_millis(50));
            }
        },
    );
}

#[test]
fn one_command_panicking_does_not_take_the_rest_down() {
    with_app(|app| {
        let state = app.state::<AppState>();
        // a command panics while it holds the progress DB
        std::thread::scope(|s| {
            let _ = s
                .spawn(|| {
                    let _held = state.user.lock().unwrap();
                    panic!("simulated bug in a command");
                })
                .join();
        });
        assert!(state.user.is_poisoned());
        assert_eq!(profile_get(app.state()).xp, 0);
        let start = session_start(app.state(), SessionKind::Quick);
        assert!(answer(app, &start, first_quiz(&start), true).combo >= 1);
    });
}

#[test]
fn mined_words_are_the_ones_in_the_sentence_not_their_relatives() {
    let srt = "1\n00:00:01,000 --> 00:00:03,000\n¿Dónde está la casa de tu madre?\n\n\
               2\n00:00:04,000 --> 00:00:06,000\nMi hermano vive en Madrid con su mujer.\n";
    with_app(move |app| {
        srt_import(app.state(), "test.srt".into(), srt.into()).unwrap();
        let id = |lemma: &str| word_search(app.state(), lemma.into(), 1)[0].word_id;
        let state = app.state::<AppState>();
        let mined = |word_id: i64| -> bool {
            state
                .user
                .lock()
                .unwrap()
                .query_row("SELECT EXISTS(SELECT 1 FROM mined_words WHERE word_id = ?1)", [word_id], |r| r.get(0))
                .unwrap()
        };
        for wanted in ["madre", "casa", "hermano", "mujer"] {
            assert!(mined(id(wanted)), "{wanted} not linked");
        }
        // "madre" is also the feminine of "padre", "casa" a form of "casar"...
        for wrong in ["padre", "casar", "hermana"] {
            assert!(!mined(id(wrong)), "{wrong} linked to a sentence that doesn't say it");
        }

        // ...and adding the sentence to the deck must not teach those instead
        let first = sentences_list(app.state(), None, None).sentences.into_iter().find(|s| s.es.contains("madre")).unwrap();
        sentence_add_to_deck(app.state(), first.id).unwrap();
        let padre = id("padre");
        let carded: bool = state
            .user
            .lock()
            .unwrap()
            .query_row("SELECT EXISTS(SELECT 1 FROM cards WHERE word_id = ?1)", [padre], |r| r.get(0))
            .unwrap();
        assert!(!carded, "adding a sentence about a mother put 'padre' in the deck");
    });
}

#[test]
fn each_session_card_says_how_many_new_words_it_brings() {
    with_app(|app| {
        let promised = |app: &tauri::App<MockRuntime>| {
            let counts = profile_get(app.state()).new_words_by_kind;
            ["quick", "standard", "deep", "review_only"].map(|k| counts.get(k).copied().expect(k))
        };
        // ...and a session started now brings exactly that many
        let brought = |app: &tauri::App<MockRuntime>| {
            [SessionKind::Quick, SessionKind::Standard, SessionKind::Deep, SessionKind::ReviewOnly]
                .map(|kind| new_word_ids(&session_start(app.state(), kind)).len() as i64)
        };
        // with no words to play games with yet, a session teaches twice as many
        assert_eq!(promised(app), [4, 6, 12, 0]);
        assert_eq!(brought(app), promised(app));

        let words = teachable(app);
        let twenty: Vec<i64> = words[..20].iter().map(|w| w.word_id).collect();
        insert_cards(app, &twenty, 1, 0);
        assert_eq!(promised(app), [2, 3, 6, 0]);
        assert_eq!(brought(app), promised(app));

        // nearly everything learned: only what is left
        let all_but_three: Vec<i64> = words[20..words.len() - 3].iter().map(|w| w.word_id).collect();
        sql(app, "BEGIN");
        insert_cards(app, &all_but_three, 2, i64::MAX);
        sql(app, "COMMIT");
        assert_eq!(promised(app), [2, 3, 3, 0]);
        assert_eq!(brought(app), promised(app));
        let last: Vec<i64> = words[words.len() - 3..].iter().map(|w| w.word_id).collect();
        insert_cards(app, &last, 2, i64::MAX);
        assert_eq!(promised(app), [0, 0, 0, 0]);
        assert_eq!(brought(app), promised(app));
    });
}

#[test]
fn spanish_first_shows_spanish_definitions() {
    with_app(|app| {
        let mut s = settings_get(app.state());
        s.definition_lang = "es".into();
        settings_set(app.state(), s).unwrap();
        let casa = word_search(app.state(), "casa".into(), 1).remove(0);
        assert_eq!(casa.gloss_es.as_deref(), Some("Edificación destinada a vivienda"));

        // an intro card's word carries it, and "es → meaning" choices offer definitions
        let start = session_start(app.state(), SessionKind::Deep);
        let spanish = start.rounds.iter().any(|r| match r {
            Round::Choice { prompt_lang, options, answer_index, .. } if prompt_lang == "es" => {
                options[*answer_index as usize].split_whitespace().count() > 2
            }
            _ => false,
        });
        assert!(spanish, "no choice round offered a Spanish definition");
    });
}

// --- storage and safety ------------------------------------------------------

/// Runs SQL on the app's own progress connection. A TEMP trigger made here
/// makes that connection's writes to a table fail, as a full or failing disk
/// would, without touching the file.
fn sql(app: &tauri::App<MockRuntime>, sql: &str) {
    app.state::<AppState>().user.lock().unwrap().execute_batch(sql).unwrap();
}

const CARDS_CANNOT_BE_WRITTEN: &str =
    "CREATE TEMP TRIGGER no_cards BEFORE INSERT ON main.cards BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;";

fn cards_in(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM cards", [], |r| r.get(0)).unwrap()
}

/// A closed progress DB at `app_dir` holding `cards` cards.
fn progress_with_cards(app_dir: &Path, cards: i64) -> Connection {
    let conn = db::open(&user_db_path(app_dir)).unwrap();
    conn.execute(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
         INSERT INTO cards(word_id, state) SELECT i, 2 FROM n WHERE ?1 > 0",
        [cards],
    )
    .unwrap();
    conn
}

/// A backup zip of some other progress with `cards` cards in it.
fn backup_with_cards(cards: i64) -> Vec<u8> {
    let dir = fresh_app_dir();
    let conn = progress_with_cards(&dir, cards);
    std::fs::read(backup::write(&conn, &dir.join("out"), backup::Kind::Manual).unwrap()).unwrap()
}

/// An app whose progress went bad while it was running: `object`'s root page
/// is unreadable, the rest of the file is fine.
fn app_with_damaged(object: &'static str) -> impl FnOnce() -> tauri::App<MockRuntime> {
    move || {
        let app_dir = fresh_app_dir();
        drop(progress_with_cards(&app_dir, 20));
        db::damage_page(&user_db_path(&app_dir), object);
        let user = db::connect(&user_db_path(&app_dir)).unwrap();
        let tts = tts::Tts::off(&app_dir);
        mock_app_over(app_dir, user, tts)
    }
}

/// A data directory whose progress is damaged, with `backups` (file name,
/// number of cards; `None` for a file that isn't a backup at all) in its
/// backup folder.
fn damaged_app_dir(backups: &[(&str, Option<i64>)]) -> PathBuf {
    let app_dir = fresh_app_dir();
    let folder = backup::folder(&app_dir);
    std::fs::create_dir_all(&folder).unwrap();
    for (name, cards) in backups {
        let bytes = cards.map(backup_with_cards).unwrap_or_else(|| b"not a zip at all".to_vec());
        std::fs::write(folder.join(name), bytes).unwrap();
    }
    drop(progress_with_cards(&app_dir, 50));
    db::damage_page(&user_db_path(&app_dir), "reviews");
    app_dir
}

#[test]
fn startup_recovers_the_newest_backup_after_setting_damaged_progress_aside() {
    let app_dir = damaged_app_dir(&[
        ("tilde-auto-20260101-120000.zip", Some(7)),
        ("tilde-auto-20260102-120000.zip", Some(3)),
    ]);
    let opened = open_progress(&app_dir).unwrap();
    let aside = opened.set_aside.expect("the damaged progress was set aside");
    assert!(aside.exists());
    let recovered = opened.recovered.expect("the newest backup was recovered");
    assert_eq!(recovered.name, "tilde-auto-20260102-120000.zip");
    assert_eq!(cards_in(&opened.conn), 3);

    let message = set_aside_message(&aside, Some(&recovered));
    assert!(message.contains(&aside.display().to_string()), "{message}");
    assert!(message.contains("Hemos recuperado tu copia de seguridad del 02/01/2026 12:00."), "{message}");
    assert!(set_aside_message(&aside, None).contains("restaura una copia en Ajustes"));

    // it is in the file, not just in that connection; and nothing is set aside twice
    drop(opened.conn);
    let again = open_progress(&app_dir).unwrap();
    assert!(again.set_aside.is_none() && again.recovered.is_none());
    assert_eq!(cards_in(&again.conn), 3);
    assert!(aside.exists());
    // nothing was used up or left lying about
    assert_eq!(backup::list(&backup::folder(&app_dir)).len(), 2);
    assert!(!app_dir.join("tilde_user.db.restore").exists());
}

#[test]
fn startup_recovery_never_undoes_a_reset() {
    for deliberate in ["tilde-before-reset-20260102-120000.zip", "tilde-before-restore-20260102-120000.zip"] {
        let app_dir = damaged_app_dir(&[("tilde-auto-20260101-120000.zip", Some(7)), (deliberate, Some(3))]);
        let opened = open_progress(&app_dir).unwrap();
        assert!(opened.set_aside.is_some());
        assert!(opened.recovered.is_none(), "recovered a backup from before {deliberate}");
        assert_eq!(cards_in(&opened.conn), 0);
    }
    // with no backups at all, it's a fresh start too
    let opened = open_progress(&damaged_app_dir(&[])).unwrap();
    assert!(opened.set_aside.is_some() && opened.recovered.is_none());
}

#[test]
fn startup_recovery_skips_a_bad_backup() {
    let app_dir = damaged_app_dir(&[
        ("tilde-backup-20260101-120000.zip", Some(7)),
        ("tilde-auto-20260102-120000.zip", None),
    ]);
    // newest of all: a real zip, but the database inside is junk
    let junk = zip_with(r#"{"app":"tilde","format":1}"#, b"SQLite format 3\0 but then nonsense");
    std::fs::write(backup::folder(&app_dir).join("tilde-auto-20260103-120000.zip"), junk).unwrap();

    let opened = open_progress(&app_dir).unwrap();
    assert_eq!(opened.recovered.map(|b| b.name).as_deref(), Some("tilde-backup-20260101-120000.zip"));
    assert_eq!(cards_in(&opened.conn), 7);
}

#[test]
fn a_restore_reaches_another_connection_to_the_same_progress() {
    with_app(|app| {
        let state = app.state::<AppState>();
        let path = user_db_path(&state.app_dir);
        // a second Tilde that slipped through, or any other program with the file open
        let other = db::connect(&path).unwrap();
        assert_eq!(cards_in(&other), 0);

        backup_import(app.state(), backup_with_cards(4)).unwrap();
        assert_eq!(count(app, "cards"), 4);
        assert_eq!(cards_in(&other), 4, "the other connection is on a file that is no longer the progress");

        // and what it writes from now on lands where everyone reads
        db::set_setting(&other, "session_length", "deep").unwrap();
        assert_eq!(settings_get(app.state()).session_length, "deep");
        let fresh = db::connect(&path).unwrap();
        assert_eq!(db::get_setting(&fresh, "session_length").as_deref(), Some("deep"));

        let mode: String = fresh.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
        assert_eq!(db::schema_version(&fresh).unwrap(), db::SCHEMA_VERSION);
    });
}

#[test]
fn a_restore_works_when_the_live_progress_is_damaged() {
    with(app_with_damaged("cards"), |app| {
        assert!(backup_export(app.state()).is_err(), "the test database isn't damaged");

        backup_import(app.state(), backup_with_cards(4)).unwrap();
        assert_eq!(count(app, "cards"), 4);
        play_one_round(app);
        assert!(count(app, "cards") > 4, "the restored progress can't be written to");

        // what was replaced is kept, marked for what it is...
        let kept = backups_of_kind(app, "before-restore");
        assert_eq!(kept.len(), 1, "{kept:?}");
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&kept[0].path).unwrap()).unwrap();
        let meta: serde_json::Value = serde_json::from_reader(zip.by_name("meta.json").unwrap()).unwrap();
        assert_eq!(meta["damaged"], true);
        // ...and can't be taken for a good backup later
        let cards = count(app, "cards");
        let err = backup_restore(app.state(), kept[0].path.clone()).expect_err("restored a damaged copy");
        assert!(err.starts_with("La copia está dañada"), "{err}");
        assert_eq!(count(app, "cards"), cards);
    });
}

#[test]
fn a_backup_with_another_page_size_restores() {
    let small_pages = {
        let path = fresh_app_dir().join("small.db");
        let mut conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA page_size = 1024;").unwrap();
        db::migrate(&mut conn).unwrap();
        for word_id in 1..=300 {
            conn.execute("INSERT INTO cards(word_id, state) VALUES (?1, 2)", [word_id]).unwrap();
        }
        let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap();
        assert_eq!(page_size, 1024);
        drop(conn);
        zip_with(r#"{"app":"tilde","format":1}"#, &std::fs::read(&path).unwrap())
    };
    with_app(move |app| {
        backup_import(app.state(), small_pages).unwrap();
        assert_eq!(count(app, "cards"), 300);
        play_one_round(app);
        assert!(count(app, "round_log") >= 1);
    });
}

#[test]
fn a_backup_from_before_versioning_restores_at_the_current_version() {
    let legacy = {
        let dir = fresh_app_dir();
        let conn = progress_with_cards(&dir, 6);
        conn.pragma_update(None, "user_version", 0).unwrap();
        conn.pragma_update(None, "journal_mode", "DELETE").unwrap();
        drop(conn);
        zip_with(r#"{"app":"tilde","format":1}"#, &std::fs::read(user_db_path(&dir)).unwrap())
    };
    with_app(move |app| {
        backup_import(app.state(), legacy).unwrap();
        assert_eq!(count(app, "cards"), 6);
        let state = app.state::<AppState>();
        assert_eq!(db::schema_version(&state.user.lock().unwrap()).unwrap(), db::SCHEMA_VERSION);
    });
}

#[test]
fn a_damaged_progress_is_never_written_as_an_automatic_backup() {
    // a table no round touches, so a session can still be played
    with(app_with_damaged("mined"), |app| {
        play_one_round(app);
        assert_eq!(count(app, "round_log"), 1, "the round wasn't graded, so no backup was due");
        assert!(backups_of_kind(app, "auto").is_empty());
        let problem = backup_folder(app.state()).last_error.expect("the failed backup is reported");
        assert!(problem.message.contains("está dañado"), "{}", problem.message);

        // the first snapshot into a newly chosen folder is an automatic one too
        let state = app.state::<AppState>();
        let elsewhere = state.app_dir.join("Sync");
        let folder = backup_folder_set(app.state(), Some(elsewhere.to_string_lossy().into_owned())).unwrap();
        assert!(folder.backups.is_empty(), "{:?}", folder.backups);
        assert!(folder.last_error.is_some());

        // nor does a reset go ahead without its safety copy
        let err = progress_reset(app.state()).expect_err("reset without a backup");
        assert!(err.contains("está dañado"), "{err}");
        assert!(count(app, "cards") > 0);
    });
}

#[test]
fn a_round_that_cannot_be_saved_reports_an_error_and_can_be_retried() {
    with_app(|app| {
        let start = session_start(app.state(), SessionKind::Standard);
        let i = start.rounds.iter().position(|r| matches!(r, Round::Choice { .. })).unwrap();
        sql(app, CARDS_CANNOT_BE_WRITTEN);
        let err = try_answer(app, &start, i, true).expect_err("the card could not be written");
        assert!(err.starts_with("No se pudo guardar tu progreso"), "{err}");
        for table in ["cards", "reviews", "events", "round_log", "badges"] {
            assert_eq!(count(app, table), 0, "{table} kept part of a round that failed");
        }

        // the same round again, once the disk is back: an answer, not a repeat
        sql(app, "DROP TRIGGER no_cards");
        let feedback = try_answer(app, &start, i, true).unwrap();
        assert!(feedback.xp_gained > 0, "the retry was taken for a repeat");
        assert_eq!(feedback.combo, 1);
        assert_eq!(answer(app, &start, i, true).xp_gained, 0, "graded twice");
        let summary = session_finish(app.state(), start.session_id);
        assert_eq!((summary.rounds, summary.correct), (1, 1), "the failed attempt was counted");
        assert_eq!(summary.xp, i64::from(feedback.xp_gained));
    });
}

#[test]
fn a_round_that_fails_at_its_last_write_is_undone_in_full() {
    with_app(|app| {
        let start = session_start(app.state(), SessionKind::Standard);
        let Round::NewWord { word, .. } = &start.rounds[0] else { panic!("first round is an intro") };
        // everything but the round's log entry can be written
        sql(app, "CREATE TEMP TRIGGER no_log BEFORE INSERT ON main.round_log BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;");
        for index in [0, 1] {
            assert!(try_answer(app, &start, index, true).is_err());
        }
        for table in ["cards", "reviews", "events", "round_log", "badges"] {
            assert_eq!(count(app, table), 0, "{table} kept part of a round that failed");
        }

        sql(app, "DROP TRIGGER no_log");
        let earned: i64 = [0, 1].iter().map(|i| i64::from(answer(app, &start, *i, true).xp_gained)).sum();
        assert!(earned > 0);
        // the session's row was made by an attempt that was rolled back; the
        // session must not go on updating a row that isn't there
        assert_eq!(profile_get(app.state()).xp, earned);
        // one intro and one answer
        let summary = session_finish(app.state(), start.session_id);
        assert_eq!((summary.rounds, summary.xp, summary.best_combo), (1, earned, 1));
        assert_eq!(summary.new_words, vec![word.lemma.clone()], "the intro was counted once per attempt");
        assert!((summary.minutes - 2.0 / 60.0).abs() < 1e-9, "the failed attempts were timed too: {}", summary.minutes);
    });
}

#[test]
fn a_round_whose_transaction_sqlite_ends_early_saves_nothing() {
    with_app(|app| {
        let start = session_start(app.state(), SessionKind::Standard);
        let quiz = start.rounds.iter().position(|r| matches!(r, Round::Choice { .. })).unwrap();
        // an earlier round is saved, so the session already has its row in `events`
        let earlier = (0..start.rounds.len()).find(|i| *i != quiz).unwrap();
        answer(app, &start, earlier, true);
        // one right answer short of the combo badge...
        app.state::<AppState>().sessions.lock().unwrap().get_mut(&start.session_id).unwrap().combo = 9;
        // ...whose insert finds the disk full. On that, SQLite ends the whole
        // transaction, not just the statement.
        sql(app, "CREATE TEMP TRIGGER disk_full BEFORE INSERT ON main.badges WHEN NEW.id = 'combo-10'
                  BEGIN SELECT RAISE(ROLLBACK, 'disk full'); END;");
        // rows per table, and the XP in `events`
        let saved = |app: &tauri::App<MockRuntime>| {
            let rows = ["cards", "reviews", "events", "round_log", "badges"].map(|table| count(app, table));
            (rows, profile_get(app.state()).xp)
        };
        let (rows_before, xp_before) = saved(app);

        let err = try_answer(app, &start, quiz, true).expect_err("the round's transaction was gone");
        assert_eq!(saved(app), (rows_before, xp_before), "part of the round was saved on its own after the rollback");
        assert_eq!(err, "No se pudo guardar tu progreso: disk full", "the first failure is the one to report");

        // once there is room again the round counts, once, badge and all
        sql(app, "DROP TRIGGER disk_full");
        let feedback = try_answer(app, &start, quiz, true).unwrap();
        assert_eq!(feedback.combo, 10);
        assert!(feedback.new_badges.iter().any(|b| b.id == "combo-10"), "{:?}", feedback.new_badges);
        let (rows, xp) = saved(app);
        assert_eq!(rows[3], rows_before[3] + 1, "the round is in the log {} times", rows[3] - rows_before[3]);
        assert_eq!(xp, xp_before + i64::from(feedback.xp_gained));
    });
}

#[test]
fn a_failing_automatic_backup_shows_in_the_backup_folder() {
    with_app(|app| {
        let state = app.state::<AppState>();
        let synced = state.app_dir.join("Sync");
        let folder = backup_folder_set(app.state(), Some(synced.to_string_lossy().into_owned())).unwrap();
        assert!(folder.last_error.is_none());

        // the folder goes away (a drive unplugged) and can't be made again
        std::fs::remove_dir_all(&synced).unwrap();
        std::fs::write(&synced, b"not a folder").unwrap();
        let before = deck::now() as i64;
        play_one_round(app);
        let problem = backup_folder(app.state()).last_error.expect("the failed backup is reported");
        assert!(problem.message.starts_with("No se pudo crear la carpeta de copias"), "{}", problem.message);
        assert!(problem.at >= before);
        // as contract.ts has it: `lastError: { message, at } | null`
        let wire = serde_json::to_value(backup_folder(app.state())).unwrap();
        assert_eq!(wire["lastError"]["message"], problem.message.as_str());
        assert_eq!(wire["lastError"]["at"], problem.at);
        // asking for one by hand fails out loud, and doesn't clear the warning
        assert!(backup_export(app.state()).is_err());
        assert!(backup_folder(app.state()).last_error.is_some());

        std::fs::remove_file(&synced).unwrap();
        play_one_round(app);
        let folder = backup_folder(app.state());
        assert!(folder.last_error.is_none(), "{:?}", folder.last_error);
        assert_eq!(folder.backups.len(), 1);
        assert!(serde_json::to_value(&folder).unwrap()["lastError"].is_null());

        // a manual backup that works, or a change of folder, clears it as well
        for clear in [0, 1] {
            std::fs::remove_dir_all(&synced).unwrap();
            std::fs::write(&synced, b"not a folder").unwrap();
            play_one_round(app);
            assert!(backup_folder(app.state()).last_error.is_some());
            std::fs::remove_file(&synced).unwrap();
            if clear == 0 {
                backup_export(app.state()).unwrap();
                assert!(backup_folder(app.state()).last_error.is_none());
            } else {
                assert!(backup_folder_set(app.state(), None).unwrap().last_error.is_none());
            }
        }
    });
}

#[test]
fn settings_that_cannot_be_saved_are_reported_and_left_as_they_were() {
    with_app(|app| {
        for event in ["INSERT", "UPDATE"] {
            sql(app, &format!(
                "CREATE TEMP TRIGGER no_sound_{event} BEFORE {event} ON main.settings WHEN NEW.key = 'sound_enabled'
                 BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;"
            ));
        }
        let mut wanted = settings_get(app.state());
        assert_eq!((wanted.definition_lang.as_str(), wanted.sound_enabled), ("en", true));
        wanted.definition_lang = "es".into();
        wanted.sound_enabled = false;
        let err = settings_set(app.state(), wanted.clone()).expect_err("one of the settings could not be written");
        assert!(err.starts_with("No se pudo guardar tu progreso"), "{err}");
        let stored = settings_get(app.state());
        assert_eq!((stored.definition_lang.as_str(), stored.sound_enabled), ("en", true), "half of the settings were saved");

        sql(app, "DROP TRIGGER no_sound_INSERT; DROP TRIGGER no_sound_UPDATE;");
        let saved = settings_set(app.state(), wanted).unwrap();
        assert_eq!((saved.definition_lang.as_str(), saved.sound_enabled), ("es", false));
    });
}

#[test]
fn an_import_that_fails_midway_leaves_nothing_behind() {
    let srt = "1\n00:00:01,000 --> 00:00:03,000\nNo quiero ir a la escuela hoy.\n\n\
               2\n00:00:04,000 --> 00:00:06,000\n¿Dónde está la casa de tu madre?\n\n\
               3\n00:00:07,000 --> 00:00:09,000\nMi hermano vive en Madrid con su mujer.\n";
    with_app(move |app| {
        // room for one more sentence, then the disk is full
        sql(app, "CREATE TEMP TRIGGER one_more BEFORE INSERT ON main.mined WHEN (SELECT COUNT(*) FROM mined) >= 1
                  BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;");
        let err = srt_import(app.state(), "test.srt".into(), srt.into()).expect_err("the second sentence could not be written");
        assert!(err.starts_with("No se pudo guardar tu progreso"), "{err}");
        for table in ["mined", "mined_words", "badges"] {
            assert_eq!(count(app, table), 0, "{table} kept part of an import that failed");
        }

        sql(app, "DROP TRIGGER one_more");
        let report = srt_import(app.state(), "test.srt".into(), srt.into()).unwrap();
        assert_eq!(report.sentences_added, 3);
        assert_eq!(count(app, "mined"), 3);
    });
}

#[test]
fn placement_that_cannot_be_saved_saves_nothing() {
    with_app(|app| {
        let answers: Vec<PlacementAnswer> = placement_start(app.state())
            .iter()
            .map(|item| PlacementAnswer { word_id: item.word_id, correct: true })
            .collect();
        // room for a few known words, then the disk is full
        sql(app, "CREATE TEMP TRIGGER no_room BEFORE INSERT ON main.cards WHEN (SELECT COUNT(*) FROM cards) >= 5
                  BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;");
        let err = placement_submit(app.state(), answers.clone()).expect_err("most known words could not be written");
        assert!(err.starts_with("No se pudo guardar tu progreso"), "{err}");
        assert!(placement_status(app.state()).is_none(), "placement was recorded as done");
        for table in ["settings", "cards", "badges"] {
            assert_eq!(count(app, table), 0, "{table} kept part of a placement that failed");
        }

        sql(app, "DROP TRIGGER no_room");
        let result = placement_submit(app.state(), answers).unwrap();
        assert!(result.words_marked_known > 0);
        assert_eq!(count(app, "cards"), result.words_marked_known);
        assert_eq!(placement_status(app.state()).map(|p| p.frontier_rank), Some(result.frontier_rank));
    });
}

#[test]
fn marking_a_word_known_reports_a_failed_write() {
    let srt = "1\n00:00:01,000 --> 00:00:03,000\n¿Dónde está la casa de tu madre?\n";
    with_app(move |app| {
        srt_import(app.state(), "test.srt".into(), srt.into()).unwrap();
        let sentence = sentences_list(app.state(), None, None).sentences.remove(0);
        let casa = word_search(app.state(), "casa".into(), 1)[0].word_id;
        word_mark_known(app.state(), casa).unwrap();

        sql(app, CARDS_CANNOT_BE_WRITTEN);
        sql(app, "CREATE TEMP TRIGGER no_forgetting BEFORE DELETE ON main.cards BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;");
        let madre = word_search(app.state(), "madre".into(), 1)[0].word_id;
        assert!(word_mark_known(app.state(), madre).is_err());
        assert!(word_reset(app.state(), casa).is_err());
        assert!(sentence_add_to_deck(app.state(), sentence.id).is_err());
        assert_eq!(count(app, "cards"), 1);
        assert!(!sentences_list(app.state(), None, None).sentences[0].in_deck, "the sentence says it is in the deck, but its words aren't");

        sql(app, "DROP TRIGGER no_cards; DROP TRIGGER no_forgetting;");
        sentence_add_to_deck(app.state(), sentence.id).unwrap();
        assert!(sentences_list(app.state(), None, None).sentences[0].in_deck);
        word_reset(app.state(), casa).unwrap();
        assert!(count(app, "cards") >= 1);
    });
}

#[test]
fn a_second_instance_cannot_take_the_lock() {
    let app_dir = fresh_app_dir();
    let first = lock_instance(&app_dir).unwrap().expect("the first instance takes the lock");
    assert!(lock_instance(&app_dir).is_err(), "two instances hold the same data directory");
    drop(first);
    // released with the process (here, the file), so a crash leaves nothing stale
    assert!(lock_instance(&app_dir).unwrap().is_some());
    // a directory that can't hold a lock file doesn't stop Tilde from starting
    assert!(lock_instance(&app_dir.join("missing")).unwrap().is_none());
}

// --- what gets taught, and when ------------------------------------------------

/// Does the placement quiz. `right` says which questions are answered
/// correctly, by how common their word is (0 = the commonest). Returns the
/// result and the questions' word ranks, commonest first.
fn place(app: &tauri::App<MockRuntime>, right: impl Fn(usize) -> bool) -> (PlacementResult, Vec<i64>) {
    let items = placement_start(app.state());
    let rank_of = |word_id: i64| app.state::<AppState>().content.lock().unwrap().word(word_id).unwrap().rank;
    let mut ranks: Vec<i64> = items.iter().map(|item| rank_of(item.word_id)).collect();
    ranks.sort();
    // answered in the order asked, which is shuffled
    let answers = items
        .iter()
        .map(|item| {
            let nth = ranks.iter().position(|r| *r == rank_of(item.word_id)).unwrap();
            PlacementAnswer { word_id: item.word_id, correct: right(nth) }
        })
        .collect();
    (placement_submit(app.state(), answers).unwrap(), ranks)
}

/// Which placement questions are answered right, by `place`'s numbering.
type Right = fn(usize) -> bool;

#[test]
fn placement_leaves_no_teachable_word_between_known_and_new() {
    let answer_sets: [Right; 4] = [|_| false, |nth| nth < 2, |nth| nth < 6, |_| true];
    for right in answer_sets {
        with_app(move |app| {
            let (result, ranks) = place(app, right);
            let words = teachable(app);
            for w in words.iter().filter(|w| w.rank < result.frontier_rank) {
                let state = card(app, w.word_id).map(|(_, state)| state);
                assert_eq!(state, Some(2), "{} (rank {}) is below the cut at {} but not known", w.lemma, w.rank, result.frontier_rank);
            }
            let below = words.iter().filter(|w| w.rank < result.frontier_rank).count() as i64;
            assert_eq!((result.words_marked_known, count(app, "cards")), (below, below), "{ranks:?}");

            // new words start right where the known ones end
            let next = words.iter().find(|w| w.rank >= result.frontier_rank).unwrap();
            let start = session_start(app.state(), SessionKind::Standard);
            let Round::NewWord { word, .. } = &start.rounds[0] else { panic!("first round is an intro") };
            assert_eq!((word.lemma.as_str(), word.rank), (next.lemma.as_str(), next.rank), "cut at {}", result.frontier_rank);
        });
    }
}

#[test]
fn a_guess_level_placement_marks_nothing() {
    // nothing right, one question right (as likely a guess as not), and the
    // quiz skipped altogether
    let answer_sets: [Option<Right>; 4] = [Some(|_| false), Some(|nth| nth == 0), Some(|nth| nth == 7), None];
    for right in answer_sets {
        with_app(move |app| {
            let result = match right {
                Some(right) => place(app, right).0,
                None => placement_submit(app.state(), vec![]).unwrap(),
            };
            assert_eq!((result.frontier_rank, result.words_marked_known, result.cefr_estimate.as_str()), (1, 0, "A1"));
            assert_eq!(count(app, "cards"), 0);
            assert!(placement_status(app.state()).is_some(), "the quiz would be offered again");
            // so the first word taught is the commonest of all
            let commonest = teachable(app).remove(0);
            let start = session_start(app.state(), SessionKind::Standard);
            assert_eq!(new_word_ids(&start).first(), Some(&commonest.word_id), "{}", commonest.lemma);
        });
    }
}

#[test]
fn placement_credit_stops_at_the_second_miss() {
    with_app(|app| {
        // by rank: 1–3 right, 4 wrong, 5 right, 6 wrong, 7–10 right
        let (result, ranks) = place(app, |nth| nth != 3 && nth != 5);
        assert_eq!(ranks.len(), 10, "{ranks:?}");
        // right answers count up to the 6th question (1, 2, 3 and 5), and the
        // cut is the second-hardest of those
        assert_eq!(result.frontier_rank, ranks[2]);
        let below = teachable(app).iter().filter(|w| w.rank < ranks[2]).count() as i64;
        assert!(below > 0);
        assert_eq!(result.words_marked_known, below);
        assert_eq!(count(app, "cards"), below);
    });
}

#[test]
fn placement_answers_are_taken_by_word_not_by_position() {
    with_app(|app| {
        let items = placement_start(app.state());
        let rank_of = |word_id: i64| app.state::<AppState>().content.lock().unwrap().word(word_id).unwrap().rank;
        let mut by_rank: Vec<i64> = items.iter().map(|item| item.word_id).collect();
        by_rank.sort_by_key(|id| rank_of(*id));
        let right = |nth: usize| PlacementAnswer { word_id: by_rank[nth], correct: true };
        let wrong = |nth: usize| PlacementAnswer { word_id: by_rank[nth], correct: false };
        // Sent hardest first; with an id that is no word; and with the
        // easiest word answered twice, where the first answer is the one
        // that counts.
        let orphan = orphan_ids(app, 1)[0];
        let answers = vec![right(3), PlacementAnswer { word_id: orphan, correct: true }, wrong(1), right(0), right(2), wrong(0)];
        let result = placement_submit(app.state(), answers).unwrap();
        // by rank: right, wrong, right, right. All three right answers
        // count, and the cut is the second-hardest of them.
        assert_eq!(result.frontier_rank, rank_of(by_rank[2]));
    });
}

#[test]
fn placement_runs_once_and_never_touches_met_words() {
    with_app(|app| {
        // the commonest word of all, so below any cut; the player has just met it
        let met = teachable(app)[0].word_id;
        user(app, |conn| deck::introduce(conn, met)).unwrap();
        assert_eq!(card(app, met), Some((0.0, 1)));

        let (first, _) = place(app, |_| true);
        assert!(first.words_marked_known > 100);
        assert_eq!(card(app, met), Some((0.0, 1)), "placement overwrote a card the player already had");
        let cards = count(app, "cards");
        assert_eq!(cards, first.words_marked_known + 1);

        // asked again, with other answers: the first result stands
        let (again, _) = place(app, |nth| nth < 3);
        assert_eq!(
            (again.frontier_rank, again.words_marked_known, again.cefr_estimate.as_str()),
            (first.frontier_rank, first.words_marked_known, first.cefr_estimate.as_str())
        );
        assert_eq!(count(app, "cards"), cards);
        assert_eq!(placement_status(app.state()).map(|p| p.frontier_rank), Some(first.frontier_rank));
    });
}

#[test]
fn placement_level_matches_home() {
    // the last two: seven right in a row, and six right after two misses.
    // By the number of right answers those look alike; by what they show
    // the player knows, they are a B2 and a beginner
    let answer_sets: [Right; 5] = [|_| true, |nth| nth < 4, |nth| nth < 2, |nth| nth < 7, |nth| nth >= 4];
    for right in answer_sets {
        with_app(move |app| {
            let (result, _) = place(app, right);
            let home = profile_get(app.state());
            assert_eq!(result.cefr_estimate, home.cefr_estimate, "{} words known", home.words_known);
            assert_eq!(result.words_marked_known, home.words_known);
        });
    }
}

#[test]
fn placement_does_not_award_learning_badges() {
    with_app(|app| {
        let (result, _) = place(app, |_| true);
        assert!(result.words_marked_known >= 1000, "{}", result.words_marked_known);
        let start = session_start(app.state(), SessionKind::Standard);
        answer(app, &start, first_quiz(&start), true);
        let summary = session_finish(app.state(), start.session_id);
        let learning: Vec<&str> = ids(&summary.new_badges).into_iter().filter(|id| id.starts_with("words-")).collect();
        assert!(learning.is_empty(), "words the quiz took as known were counted as learned: {learning:?}");
        assert!(earned(app).iter().all(|id| !id.starts_with("words-")));
    });
}

#[test]
fn learning_badges_count_answered_words() {
    with_app(|app| {
        let fifty: Vec<i64> = teachable(app)[..50].iter().map(|w| w.word_id).collect();
        insert_cards(app, &fifty, 2, i64::MAX);
        assert_eq!(profile_get(app.state()).words_known, 50);
        // a session that answers nothing about them
        let finish = |app: &tauri::App<MockRuntime>| {
            let start = session_start(app.state(), SessionKind::Standard);
            let intro = start.rounds.iter().position(|r| matches!(r, Round::NewWord { .. })).unwrap();
            answer(app, &start, intro, true);
            session_finish(app.state(), start.session_id)
        };
        // known, but never answered here (marked known, or taken as known by placement)
        assert!(!ids(&finish(app).new_badges).contains(&"words-50"));

        user(app, |conn| {
            for id in &fifty[..49] {
                conn.execute("INSERT INTO reviews(word_id, ts, grade) VALUES (?1, 0, 3)", [id]).unwrap();
            }
        });
        assert!(!ids(&finish(app).new_badges).contains(&"words-50"), "49 words learned");
        // several answers to one word are still one word
        user(app, |conn| {
            conn.execute("INSERT INTO reviews(word_id, ts, grade) VALUES (?1, 0, 3), (?1, 1, 3), (?2, 0, 3)", [fifty[0], fifty[49]])
        })
        .unwrap();
        let badges = finish(app).new_badges;
        assert!(ids(&badges).contains(&"words-50"), "{:?}", ids(&badges));
        assert!(!ids(&badges).contains(&"words-200"));
    });
}

#[test]
fn marking_an_introduced_word_known_survives_a_correct_answer() {
    with_app(|app| {
        let word = teachable(app)[0].word_id;
        user(app, |conn| deck::introduce(conn, word)).unwrap();
        word_mark_known(app.state(), word).unwrap();
        assert_eq!(profile_get(app.state()).words_known, 1);

        user(app, |conn| deck::grade(conn, word, 3)).unwrap();
        let (stability, state) = card(app, word).unwrap();
        assert!(stability >= 120.0 && state == 2, "one right answer took a known word back to stability {stability}, state {state}");
        assert_eq!(profile_get(app.state()).words_known, 1);
    });
}

#[test]
fn cards_broken_by_the_old_mark_known_are_respected() {
    with_app(|app| {
        let word = teachable(app)[0].word_id;
        // what "mark as known" on an introduced word used to leave behind
        user(app, |conn| {
            conn.execute(
                "INSERT INTO cards(word_id, stability, difficulty, due_ts, last_review_ts, reps, lapses, state)
                 VALUES (?1, 120.0, 5.0, ?2, NULL, 0, 0, 2)",
                rusqlite::params![word, deck::now() + 180.0 * 86400.0],
            )
        })
        .unwrap();
        user(app, |conn| deck::grade(conn, word, 3)).unwrap();
        let (stability, _) = card(app, word).unwrap();
        assert!(stability >= 120.0, "graded as if never seen: stability {stability}");
        assert_eq!(profile_get(app.state()).words_known, 1);
    });
}

#[test]
fn marking_known_never_weakens_a_card() {
    with_app(|app| {
        let word = teachable(app)[0].word_id;
        let due = deck::now() + 900.0 * 86400.0;
        user(app, |conn| {
            conn.execute(
                "INSERT INTO cards(word_id, stability, difficulty, due_ts, last_review_ts, reps, lapses, state)
                 VALUES (?1, 400.0, 2.5, ?2, 5, 9, 1, 2)",
                rusqlite::params![word, due],
            )
        })
        .unwrap();
        word_mark_known(app.state(), word).unwrap();
        let row: (f64, f64, f64, i64, i64) = user(app, |conn| {
            conn.query_row("SELECT stability, difficulty, due_ts, reps, lapses FROM cards WHERE word_id = ?1", [word], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
        })
        .unwrap();
        assert_eq!(row, (400.0, 2.5, due, 9, 1));

        // and a word only just introduced gets the whole of a known card
        let fresh = teachable(app)[1].word_id;
        user(app, |conn| deck::introduce(conn, fresh)).unwrap();
        word_mark_known(app.state(), fresh).unwrap();
        let row: (f64, f64, i64, bool) = user(app, |conn| {
            conn.query_row(
                "SELECT stability, difficulty, state, last_review_ts IS NOT NULL AND due_ts > ?2 FROM cards WHERE word_id = ?1",
                rusqlite::params![fresh, deck::now() + 179.0 * 86400.0],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
        })
        .unwrap();
        assert_eq!(row, (120.0, 4.5, 2, true));
    });
}

#[test]
fn explore_and_home_agree_on_known() {
    with_app(|app| {
        let casa = word_search(app.state(), "casa".into(), 1)[0].word_id;
        let in_explore = |app: &tauri::App<MockRuntime>| {
            let hit = word_search(app.state(), "casa".into(), 1).remove(0);
            assert_eq!(word_detail(app.state(), casa).unwrap().word.known, hit.known);
            hit.known
        };
        // met, and answered right once: learning it, not known yet
        user(app, |conn| deck::introduce(conn, casa).and_then(|()| deck::grade(conn, casa, 3))).unwrap();
        assert_eq!(card(app, casa).map(|(_, state)| state), Some(2));
        assert_eq!((in_explore(app), profile_get(app.state()).words_known), (false, 0));

        word_mark_known(app.state(), casa).unwrap();
        assert_eq!((in_explore(app), profile_get(app.state()).words_known), (true, 1));
    });
}

#[test]
fn a_beginner_session_quizzes_only_words_it_introduced() {
    for (kind, intros, has_match) in [(SessionKind::Quick, 4, false), (SessionKind::Standard, 6, true), (SessionKind::Deep, 12, true)] {
        with_app(move |app| {
            let start = session_start(app.state(), kind);
            let mut met = HashSet::new();
            for (i, round) in start.rounds.iter().enumerate() {
                if let Round::NewWord { word, .. } = round {
                    met.insert(word.word_id);
                    // its first quiz comes straight after
                    assert_eq!(quizzed(&start.rounds[i + 1]), [word.word_id]);
                }
                for word_id in quizzed(round) {
                    assert!(met.contains(&word_id), "{kind:?}: round {i} asks about a word never shown: {round:?}");
                }
            }
            assert_eq!(met.len(), intros, "{kind:?}");
            assert_eq!(start.rounds.iter().any(|r| matches!(r, Round::Match { .. })), has_match, "{kind:?}");
            // every word introduced is played with at least once more
            for word_id in &met {
                let asked = start.rounds.iter().filter(|r| quizzed(r).contains(word_id)).count();
                assert!(asked >= 2, "{kind:?}: word {word_id} asked {asked} time(s)");
            }
        });
    }
}

#[test]
fn sidecar_quizzes_only_words_already_met() {
    with_app(|app| {
        // no intro cards next to a film, so with no words yet there is nothing to ask
        let empty = session_start(app.state(), SessionKind::Sidecar);
        assert!(empty.rounds.is_empty());
        // and the empty session finishes cleanly, earning nothing
        assert!(session_finish(app.state(), empty.session_id).new_badges.is_empty());
        assert_eq!(count(app, "events"), 0);

        let five: Vec<i64> = teachable(app)[..5].iter().map(|w| w.word_id).collect();
        user(app, |conn| five.iter().try_for_each(|id| deck::introduce(conn, *id))).unwrap();
        let start = session_start(app.state(), SessionKind::Sidecar);
        let mut asked: Vec<i64> = start.rounds.iter().flat_map(quizzed).collect();
        asked.sort();
        let mut expected = five.clone();
        expected.sort();
        assert_eq!(asked, expected, "{:?}", start.rounds);
    });
}

#[test]
fn an_intro_card_can_be_answered_i_already_know_it() {
    with_app(|app| {
        let start = session_start(app.state(), SessionKind::Standard);
        let lemma = |i: usize| match &start.rounds[i] {
            Round::NewWord { word, .. } => (word.word_id, word.lemma.clone()),
            other => panic!("round {i} is not an intro: {other:?}"),
        };
        let (known_id, known_lemma) = lemma(0);
        let (new_id, new_lemma) = lemma(2);
        // "Ya me la sé" on the first card, "¡Lo tengo!" on the second
        let feedback = submit(app, &start, 0, true, 1000, Some(4)).unwrap();
        assert_eq!((feedback.xp_gained, feedback.combo), (0, 0));
        submit(app, &start, 2, true, 1000, None).unwrap();
        assert_eq!(card(app, known_id), Some((120.0, 2)));
        assert_eq!(card(app, new_id), Some((0.0, 1)));
        assert!(word_detail(app.state(), known_id).unwrap().word.known);

        // its quiz still comes, and a right answer leaves it known
        answer(app, &start, 1, true);
        assert!(card(app, known_id).unwrap().0 >= 120.0);
        let summary = session_finish(app.state(), start.session_id);
        assert_eq!(summary.new_words, vec![new_lemma], "{known_lemma} was listed as a new word");
        assert_eq!(profile_get(app.state()).words_known, 1);
        // and it is not offered as new again
        assert!(!new_word_ids(&session_start(app.state(), SessionKind::Standard)).contains(&known_id));
    });
}

#[test]
fn minutes_count_active_time_capped_per_round() {
    with_app(|app| {
        let start = session_start(app.state(), SessionKind::Standard);
        // the window sat on one round for an hour; the next took a second
        submit(app, &start, 0, true, 3_600_000, None).unwrap();
        submit(app, &start, 1, true, 1000, None).unwrap();
        // a clock that went backwards adds nothing, and takes nothing away
        submit(app, &start, 2, true, -5000, None).unwrap();
        // a second submit of an answered round isn't more time played
        submit(app, &start, 1, true, 60_000, None).unwrap();
        let duration_s = |app: &tauri::App<MockRuntime>| -> i64 {
            user(app, |conn| conn.query_row("SELECT SUM(duration_s) FROM events", [], |r| r.get(0))).unwrap()
        };
        assert_eq!(duration_s(app), 121);

        let summary = session_finish(app.state(), start.session_id);
        assert_eq!(summary.minutes, 121.0 / 60.0);
        assert_eq!(duration_s(app), 121);
        assert_eq!(profile_get(app.state()).minutes_total, 2);
    });
}

#[test]
fn summary_and_home_count_the_same_seconds() {
    with_app(|app| {
        let start = session_start(app.state(), SessionKind::Standard);
        for index in 0..5 {
            submit(app, &start, index, true, 61_000, None).unwrap();
        }
        // 999 ms more is not another second, here or there
        submit(app, &start, 5, true, 999, None).unwrap();
        let summary = session_finish(app.state(), start.session_id);
        assert_eq!(summary.minutes, 305.0 / 60.0);
        assert_eq!(summary.minutes.floor() as i64, profile_get(app.state()).minutes_total);
        assert_eq!(profile_get(app.state()).minutes_total, 5);
    });
}

#[test]
fn intro_cards_are_not_answers() {
    with_app(|app| {
        // every intro seen, every question missed
        let start = session_start(app.state(), SessionKind::Standard);
        let mut questions = 0;
        for (i, round) in start.rounds.iter().enumerate() {
            let intro = matches!(round, Round::NewWord { .. });
            questions += i64::from(!intro);
            assert_eq!(answer(app, &start, i, intro).combo, 0, "round {i}");
        }
        let summary = session_finish(app.state(), start.session_id);
        assert_eq!((summary.rounds, summary.correct, summary.best_combo), (questions, 0, 0));
        assert_eq!(summary.accuracy, 0.0);
        assert_eq!(summary.new_words.len() as i64, start.rounds.len() as i64 - questions);
    });
    with_app(|app| {
        // an intro between two right answers doesn't break the run, whatever
        // the card reports
        let start = session_start(app.state(), SessionKind::Quick);
        let combos: Vec<i32> = (0..4).map(|i| answer(app, &start, i, i != 2).combo).collect();
        assert_eq!(combos, [0, 1, 1, 2], "{:?}", &start.rounds[..4]);
        // Four intros and their four questions, all right: four answers, and
        // "a session without a single miss" asks for five.
        for i in 4..8 {
            answer(app, &start, i, true);
        }
        let summary = session_finish(app.state(), start.session_id);
        assert_eq!((summary.rounds, summary.correct, summary.best_combo, summary.accuracy), (4, 4, 4, 1.0));
        assert!(!ids(&summary.new_badges).contains(&"perfect-session"), "{:?}", ids(&summary.new_badges));
    });
}

#[test]
fn an_abandoned_session_is_dropped_when_a_new_one_starts() {
    with_app(|app| {
        let live = |app: &tauri::App<MockRuntime>| {
            let mut ids: Vec<i64> = app.state::<AppState>().sessions.lock().unwrap().keys().copied().collect();
            ids.sort();
            ids
        };
        let abandoned = session_start(app.state(), SessionKind::Standard);
        answer(app, &abandoned, 0, true);
        // a long session still being played: started long ago, answered just now
        let long = session_start(app.state(), SessionKind::Deep);
        assert_eq!(live(app), [abandoned.session_id, long.session_id]);
        app.state::<AppState>().sessions.lock().unwrap().get_mut(&abandoned.session_id).unwrap().last_active -= 7.0 * 3600.0;

        let next = session_start(app.state(), SessionKind::Quick);
        assert_eq!(live(app), [long.session_id, next.session_id]);
        // what the abandoned one earned was saved as it went
        assert_eq!(profile_get(app.state()).sessions_total, 1);
        // answering it now is like answering any session that is gone
        assert_eq!(answer(app, &abandoned, 1, true).xp_gained, 0);

        // an answer keeps a session alive however long ago it started
        app.state::<AppState>().sessions.lock().unwrap().get_mut(&long.session_id).unwrap().last_active -= 5.9 * 3600.0;
        answer(app, &long, 0, true);
        app.state::<AppState>().sessions.lock().unwrap().get_mut(&long.session_id).unwrap().last_active -= 5.9 * 3600.0;
        let third = session_start(app.state(), SessionKind::Quick);
        assert_eq!(live(app), [long.session_id, next.session_id, third.session_id]);
    });
}

// --- the first-session badge ---------------------------------------------------

#[test]
fn placement_awards_no_badge() {
    with_app(|app| {
        place(app, |_| true);
        assert!(earned(app).is_empty(), "{:?}", earned(app));
    });
}

#[test]
fn first_session_badge_comes_with_the_first_finished_session() {
    with_app(|app| {
        let start = session_start(app.state(), SessionKind::Quick);
        // the first new word's card, then the first question about it
        for index in 0..=first_quiz(&start) {
            let feedback = answer(app, &start, index, true);
            assert!(feedback.new_badges.is_empty(), "{:?}", ids(&feedback.new_badges));
        }
        assert!(earned(app).is_empty(), "awarded before the session was finished");
        let summary = session_finish(app.state(), start.session_id);
        assert_eq!(ids(&summary.new_badges), ["first-session"]);
        assert_eq!(earned(app), ["first-session"]);

        let second = session_start(app.state(), SessionKind::Quick);
        for index in 0..=first_quiz(&second) {
            answer(app, &second, index, true);
        }
        assert!(session_finish(app.state(), second.session_id).new_badges.is_empty());
    });
}

#[test]
fn finishing_an_untouched_session_awards_nothing_and_writes_no_backup() {
    with_app(|app| {
        let start = session_start(app.state(), SessionKind::Standard);
        let summary = session_finish(app.state(), start.session_id);
        assert!(summary.new_badges.is_empty(), "{:?}", ids(&summary.new_badges));
        assert_eq!((summary.rounds, summary.xp, summary.minutes), (0, 0, 0.0));
        assert!(earned(app).is_empty());
        assert!(backup_folder(app.state()).backups.is_empty());
        assert_eq!(profile_get(app.state()).sessions_total, 0);
    });
}

#[test]
fn intro_cards_alone_are_backed_up_but_are_not_a_session_finished() {
    with_app(|app| {
        // Every new word looked at and no question answered. The words are in
        // the deck now, so there is something to back up...
        let start = session_start(app.state(), SessionKind::Standard);
        let intros: Vec<usize> = (0..start.rounds.len()).filter(|i| matches!(start.rounds[*i], Round::NewWord { .. })).collect();
        assert!(!intros.is_empty());
        for index in &intros {
            answer(app, &start, *index, true);
        }
        let summary = session_finish(app.state(), start.session_id);
        assert_eq!(count(app, "cards"), intros.len() as i64);
        assert_eq!(backups_of_kind(app, "auto").len(), 1);
        // ...but a summary of no rounds is not "Termina tu primera sesión"
        assert_eq!(summary.rounds, 0);
        assert!(summary.new_badges.is_empty(), "{:?}", ids(&summary.new_badges));
        assert!(earned(app).is_empty());

        // one answer makes it a session, right or wrong
        let start = session_start(app.state(), SessionKind::Standard);
        let quiz = first_quiz(&start);
        for index in 0..quiz {
            answer(app, &start, index, true);
        }
        answer(app, &start, quiz, false);
        let summary = session_finish(app.state(), start.session_id);
        assert_eq!((summary.rounds, summary.correct), (1, 0));
        assert_eq!(ids(&summary.new_badges), ["first-session"]);
    });
}

// --- cards whose word has left the dictionary ----------------------------------

#[test]
fn committed_content_matches_the_word_id_registry() {
    // Progress is keyed by word id, and the registry is what keeps a word's
    // id the same from one build of the dictionary to the next. A dictionary
    // committed without its registry (or a registry edited by hand) would
    // let the next build hand those ids to other words.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/tilde-pipeline/curation/word_ids.tsv");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut registered: HashMap<&str, i64> = HashMap::new();
    let mut used = HashSet::new();
    for line in text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (lemma, id) = line.split_once('\t').unwrap_or_else(|| panic!("no tab in {line:?}"));
        let id: i64 = id.parse().unwrap_or_else(|_| panic!("bad id in {line:?}"));
        assert!(registered.insert(lemma, id).is_none(), "{lemma} is registered twice");
        assert!(used.insert(id), "id {id} is registered twice");
    }
    let content = ContentDb::open(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/content.db"))).unwrap();
    let mut stmt = content.conn.prepare("SELECT lemma, id FROM words").unwrap();
    let words: Vec<(String, i64)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    assert!(!words.is_empty());
    let mut wrong = words.iter().filter(|(lemma, id)| registered.get(lemma.as_str()) != Some(id));
    // by id only: this is no place to print a word list
    assert_eq!(wrong.next().map(|(_, id)| id), None, "a word's id in content.db is not its id in word_ids.tsv");
}

#[test]
fn orphan_cards_do_not_take_review_slots() {
    with_app(|app| {
        // more of them than a review session has rounds, all due before the real one
        insert_cards(app, &orphan_ids(app, 25), 2, 0);
        let real = teachable(app)[0].word_id;
        insert_cards(app, &[real], 2, 1);

        let start = session_start(app.state(), SessionKind::ReviewOnly);
        let asked: Vec<i64> = start.rounds.iter().flat_map(quizzed).collect();
        assert_eq!(asked, [real]);
        assert!(matches!(start.rounds[0], Round::ReviewCard { .. }));
        // a mixed session's review cards, and the sidecar's words
        for kind in [SessionKind::Standard, SessionKind::Sidecar] {
            let start = session_start(app.state(), kind);
            assert!(start.rounds.iter().flat_map(quizzed).any(|id| id == real), "{kind:?}");
        }
        assert_eq!(count(app, "cards"), 26, "the dormant cards are kept");
    });
}

#[test]
fn orphan_learning_cards_do_not_push_out_learning_words() {
    with_app(|app| {
        // more than a session looks at, all ahead of the real ones in due order
        insert_cards(app, &orphan_ids(app, 60), 1, 0);
        let real: Vec<i64> = teachable(app)[..2].iter().map(|w| w.word_id).collect();
        user(app, |conn| real.iter().try_for_each(|id| deck::introduce(conn, *id))).unwrap();

        let start = session_start(app.state(), SessionKind::Standard);
        let new = new_word_ids(&start);
        // the games, not the review cards (those come from the due list)
        let games = start.rounds.iter().filter(|r| !matches!(r, Round::ReviewCard { .. }));
        let played: HashSet<i64> = games.flat_map(quizzed).collect();
        for id in &real {
            assert!(played.contains(id), "a word in the deck got no game: {id}");
        }
        // the games are played with those and the session's new words, no others
        let expected: HashSet<i64> = real.iter().chain(&new).copied().collect();
        assert_eq!(played, expected);
        // Two words in the deck are not sixty-two: there are still too few to
        // fill the games, so the session brings extra new words, as Home says.
        assert_eq!(new.len(), 6);
        assert_eq!(profile_get(app.state()).new_words_by_kind.get("standard"), Some(&6));
    });
}

#[test]
fn orphan_cards_are_not_counted() {
    with_app(|app| {
        let real: Vec<i64> = teachable(app)[..3].iter().map(|w| w.word_id).collect();
        insert_cards(app, &real, 2, 0);
        let numbers = |app: &tauri::App<MockRuntime>| {
            let p = profile_get(app.state());
            (p.reviews_due, p.words_learning, p.words_known, p.cefr_estimate)
        };
        let before = numbers(app);
        assert_eq!(before, (3, 3, 3, "A1".to_string()));

        // enough to change the level estimate and earn a badge, were they counted
        let orphans = orphan_ids(app, 130);
        insert_cards(app, &orphans, 2, 0);
        user(app, |conn| {
            for id in &orphans {
                conn.execute("INSERT INTO reviews(word_id, ts, grade) VALUES (?1, 0, 3)", [id]).unwrap();
            }
        });
        assert_eq!(numbers(app), before);

        let start = session_start(app.state(), SessionKind::Standard);
        // a new word's card and the question that follows it
        let intro = start.rounds.iter().position(|r| matches!(r, Round::NewWord { .. })).unwrap();
        assert!(matches!(start.rounds[intro + 1], Round::Choice { .. }));
        answer(app, &start, intro, true);
        answer(app, &start, intro + 1, true);
        let summary = session_finish(app.state(), start.session_id);
        assert_eq!(ids(&summary.new_badges), ["first-session"]);
        // the stats chart's count, saved with the session
        let meta: String = user(app, |conn| conn.query_row("SELECT meta FROM events", [], |r| r.get(0))).unwrap();
        assert_eq!(serde_json::from_str::<EventMeta>(&meta).unwrap().words_known, 3);
        // and none was handed to placement as a word already met... nor lost
        assert_eq!(user(app, deck::card_ids).len(), 3 + 130 + 1);
    });
}

#[test]
fn marking_an_unknown_word_known_does_nothing() {
    with_app(|app| {
        let orphans = orphan_ids(app, 2);
        word_mark_known(app.state(), orphans[0]).unwrap();
        assert_eq!(count(app, "cards"), 0, "a card was made for a word that doesn't exist");

        // a dormant card isn't deleted by id either: its word may come back
        insert_cards(app, &orphans[1..], 2, 0);
        word_reset(app.state(), orphans[1]).unwrap();
        assert_eq!(card(app, orphans[1]), Some((100.0, 2)));
        assert!(word_detail(app.state(), orphans[1]).is_none());

        // real words are marked and reset as ever
        let real = teachable(app)[0].word_id;
        word_mark_known(app.state(), real).unwrap();
        assert_eq!(card(app, real), Some((120.0, 2)));
        word_reset(app.state(), real).unwrap();
        assert_eq!(card(app, real), None);
    });
}

#[test]
fn adding_a_sentence_skips_words_not_in_content() {
    with_app(|app| {
        let real = teachable(app)[0].word_id;
        let orphan = orphan_ids(app, 1)[0];
        user(app, |conn| {
            conn.execute_batch(&format!(
                "INSERT INTO mined(id, es, created_at) VALUES (7, 'Una frase de antes.', 0);
                 INSERT INTO mined_words(mined_id, word_id) VALUES (7, {real}), (7, {orphan});"
            ))
        })
        .unwrap();
        sentence_add_to_deck(app.state(), 7).unwrap();
        assert_eq!(card(app, real), Some((0.0, 1)));
        assert_eq!(card(app, orphan), None, "a card was made for a word that doesn't exist");
        assert!(sentences_list(app.state(), None, None).sentences[0].in_deck);
    });
}

// --- stats, Explorar and Frases ---

#[test]
fn stats_days_are_local_and_keep_the_year() {
    use chrono::{FixedOffset, TimeZone, Utc};
    let at = |y, mo, d, h, mi| Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap().timestamp();
    let labels = |points: &[SeriesPoint]| points.iter().map(|p| p.label.clone()).collect::<Vec<_>>();

    // the same day a year apart is two points, told apart by the year
    let years = words_by_day(&[(at(2025, 10, 5, 12, 0), 40), (at(2026, 10, 5, 12, 0), 90)], &Utc);
    assert_eq!(labels(&years), ["2025-10-05", "2026-10-05"]);

    // 23:30 UTC is already tomorrow in a Madrid summer (UTC+2)
    let madrid = FixedOffset::east_opt(2 * 3600).unwrap();
    let rows = [
        (at(2026, 10, 5, 12, 0), 10),
        (at(2026, 10, 5, 23, 30), 12),
        (at(2026, 10, 6, 9, 0), 15),
    ];
    let days = words_by_day(&rows, &madrid);
    assert_eq!(labels(&days), ["2026-10-05", "2026-10-06"]);
    // each day shows its last count, not its first
    assert_eq!(days.iter().map(|p| p.value).collect::<Vec<_>>(), [10.0, 15.0]);

    // a later moment with an earlier date (clocks put back across midnight)
    // doesn't make the chart turn round
    let back = words_by_day(&[(at(2026, 10, 6, 0, 10), 20), (at(2026, 10, 5, 23, 40), 21)], &Utc);
    assert_eq!(labels(&back), ["2026-10-06"]);
    assert_eq!(back[0].value, 21.0);
}

#[test]
fn accuracy_is_unknown_before_any_graded_round() {
    with_app(|app| {
        assert_eq!(stats_get(app.state()).accuracy_all_time, None);
        // an intro card is not an answer
        sql(app, "INSERT INTO round_log(ts, session_id, round_type, correct) VALUES (1, 1, 'new_word', 1)");
        assert_eq!(stats_get(app.state()).accuracy_all_time, None);
        // ...and one wrong answer is 0%, which is not the same as nothing
        sql(app, "INSERT INTO round_log(ts, session_id, round_type, correct) VALUES (2, 1, 'choice', 0)");
        assert_eq!(stats_get(app.state()).accuracy_all_time, Some(0.0));
    });
}

#[test]
fn sentences_list_pages_newest_first_with_a_total() {
    with_app(|app| {
        let mine = |n: i64| {
            sql(app, &format!(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {n})
                 INSERT INTO mined(es, source, created_at) SELECT 'Frase ' || i, 'test.srt', i FROM n"
            ));
        };
        mine(250);
        let ids = |page: &tilde_core::types::SentencePage| page.sentences.iter().map(|s| s.id).collect::<Vec<_>>();
        let mut seen = Vec::new();
        let mut before = None;
        for expected in [151..=250, 51..=150, 1..=50] {
            let page = sentences_list(app.state(), before, None);
            assert_eq!(page.total, 250);
            assert_eq!(ids(&page), expected.rev().collect::<Vec<_>>());
            before = page.sentences.last().map(|s| s.id);
            seen.extend(ids(&page));
        }
        assert_eq!(seen, (1..=250).rev().collect::<Vec<_>>());
        let past_the_end = sentences_list(app.state(), before, None);
        assert!(past_the_end.sentences.is_empty());
        assert_eq!(past_the_end.total, 250);

        // sentences mined between two pages move nothing: the next page
        // still starts right below the last one seen
        mine(300);
        assert_eq!(ids(&sentences_list(app.state(), Some(151), None)), (51..=150).rev().collect::<Vec<_>>());

        // the page size has a floor and a ceiling
        let state = app.state::<AppState>();
        let conn = state.user.lock().unwrap();
        assert_eq!(ids(&mined_page(&conn, None, Some(0))), [550]);
        let all = mined_page(&conn, None, Some(100_000));
        assert_eq!((all.sentences.len(), all.total), (500, 550));

        let wire = serde_json::to_value(&all).unwrap();
        assert!(wire["sentences"][0]["inDeck"].is_boolean() && wire["total"] == 550, "{wire}");
    });
}

#[test]
fn word_hit_marks_cards_in_progress() {
    with_app(|app| {
        let casa = || word_search(app.state(), "casa".into(), 1).remove(0);
        let word = casa();
        assert!(!word.in_deck && !word.known);

        {
            let state = app.state::<AppState>();
            let conn = state.user.lock().unwrap();
            deck::introduce(&conn, word.word_id).unwrap();
        }
        let learning = casa();
        assert!(learning.in_deck, "a word with a card is in the deck");
        assert!(!learning.known, "a word just met is not known yet");
        assert_eq!(serde_json::to_value(&learning).unwrap()["inDeck"], true);
        assert!(word_detail(app.state(), word.word_id).unwrap().word.in_deck);

        word_mark_known(app.state(), word.word_id).unwrap();
        let known = casa();
        assert!(known.in_deck && known.known);

        word_reset(app.state(), word.word_id).unwrap();
        assert!(!casa().in_deck);
    });
}

/// Field names of an `export interface` in `src/lib/contract.ts`.
fn ts_interface_keys(name: &str) -> Vec<String> {
    let ts = include_str!("../../src/lib/contract.ts");
    let start = ts
        .find(&format!("export interface {name} {{"))
        .unwrap_or_else(|| panic!("no interface {name} in contract.ts"));
    let body = &ts[start..start + ts[start..].find("\n}").unwrap()];
    let mut keys: Vec<String> = body
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|l| !l.starts_with("/*") && !l.starts_with('*') && !l.starts_with("//"))
        .filter_map(|l| l.split_once(':'))
        .map(|(k, _)| k.trim_end_matches('?').to_string())
        .collect();
    keys.sort();
    keys
}

#[test]
fn stats_word_and_sentence_wire_formats_match_contract_ts() {
    // Each sample is what a command returns, not a struct written out here:
    // a field added to one of these types changes contract.ts, not this test.
    with_app(|app| {
        // as after an automatic backup that failed, so that `lastError` is filled in
        app.state::<AppState>().backup_failed("disk on fire".into());
        let folder = serde_json::to_value(backup_folder(app.state())).unwrap();
        let samples = [
            ("WordHit", serde_json::to_value(word_search(app.state(), "casa".into(), 1).remove(0)).unwrap()),
            ("SentencePage", serde_json::to_value(sentences_list(app.state(), None, None)).unwrap()),
            ("StatsData", serde_json::to_value(stats_get(app.state())).unwrap()),
            ("BackupProblem", folder["lastError"].clone()),
            ("BackupFolder", folder),
        ];
        for (name, value) in samples {
            let fields = value.as_object().unwrap_or_else(|| panic!("{name} is not an object: {value}"));
            let mut keys: Vec<String> = fields.keys().cloned().collect();
            keys.sort();
            assert_eq!(keys, ts_interface_keys(name), "{name} in types.rs and in contract.ts");
        }
    });
}
