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
        (report, sentences_list(app.state()))
    });
    assert_eq!(report.sentences_found, 3);
    assert!(report.sentences_added > 0, "{report:?}");
    assert!(!report.top_words.is_empty());
    assert_eq!(listed.len() as i64, report.sentences_added);

    // the same file again adds nothing new
    let again = with_app(move |app| {
        srt_import(app.state(), "test.srt".into(), srt.into()).unwrap();
        let second = srt_import(app.state(), "test.srt".into(), srt.into()).unwrap();
        (second.sentences_added, sentences_list(app.state()).len())
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
        (report.sentences_added, sentences_list(app.state()))
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
        sentences_list(app.state())
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
        assert_eq!(sentences_list(app.state())[0].es, "Mi madre est\u{e1} en casa.");
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
    let result = RoundResult {
        round_index: index,
        correct,
        duration_ms: 1000,
        quality: None,
        missed_word_ids: vec![],
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
            content.new_candidates(1, 300, &HashSet::new())
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
        assert_eq!(labels, ["-7w", "-6w", "-5w", "-4w", "-3w", "-2w", "-1w", "now"]);
        let minutes: Vec<f64> = stats.minutes_per_week.iter().map(|p| p.value).collect();
        assert_eq!(minutes, [0.0, 0.0, 0.0, 0.0, 0.0, 20.0, 0.0, 10.0]);
        assert_eq!(stats.xp_per_week.last().map(|p| p.value), Some(50.0));
        // intros aren't answers: they count in neither accuracy nor the game mix
        assert!((stats.accuracy_all_time - 0.5).abs() < 1e-6, "{}", stats.accuracy_all_time);
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
        assert!(answer(app, &start, 0, true).combo >= 1);
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
        let first = sentences_list(app.state()).into_iter().find(|s| s.es.contains("madre")).unwrap();
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
        let counts = profile_get(app.state()).new_words_by_kind;
        let n = |k: &str| counts.get(k).copied();
        assert_eq!((n("quick"), n("standard"), n("deep"), n("review_only")), (Some(2), Some(3), Some(6), Some(0)));
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
        let summary = session_finish(app.state(), start.session_id);
        assert_eq!((summary.rounds, summary.xp, summary.best_combo), (2, earned, 2));
        assert_eq!(summary.new_words, vec![word.lemma.clone()], "the intro was counted once per attempt");
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
        let sentence = sentences_list(app.state()).remove(0);
        let casa = word_search(app.state(), "casa".into(), 1)[0].word_id;
        word_mark_known(app.state(), casa).unwrap();

        sql(app, CARDS_CANNOT_BE_WRITTEN);
        sql(app, "CREATE TEMP TRIGGER no_forgetting BEFORE DELETE ON main.cards BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;");
        let madre = word_search(app.state(), "madre".into(), 1)[0].word_id;
        assert!(word_mark_known(app.state(), madre).is_err());
        assert!(word_reset(app.state(), casa).is_err());
        assert!(sentence_add_to_deck(app.state(), sentence.id).is_err());
        assert_eq!(count(app, "cards"), 1);
        assert!(!sentences_list(app.state())[0].in_deck, "the sentence says it is in the deck, but its words aren't");

        sql(app, "DROP TRIGGER no_cards; DROP TRIGGER no_forgetting;");
        sentence_add_to_deck(app.state(), sentence.id).unwrap();
        assert!(sentences_list(app.state())[0].in_deck);
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
