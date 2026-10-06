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

fn mock_app_speaking(tts: impl FnOnce(&std::path::Path) -> tts::Tts) -> tauri::App<MockRuntime> {
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
    )))
    .unwrap();
    let user = db::open(&app_dir.join("tilde_user.db")).unwrap();
    let tts = Arc::new(tts(&app_dir));
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
        let report = srt_import(app.state(), "test.srt".into(), srt.into());
        (report, sentences_list(app.state()))
    });
    assert_eq!(report.sentences_found, 3);
    assert!(report.sentences_added > 0, "{report:?}");
    assert!(!report.top_words.is_empty());
    assert_eq!(listed.len() as i64, report.sentences_added);

    // the same file again adds nothing new
    let again = with_app(move |app| {
        srt_import(app.state(), "test.srt".into(), srt.into());
        let second = srt_import(app.state(), "test.srt".into(), srt.into());
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
        srt_import(app.state(), "uno.srt".into(), first.into());
        let report = srt_import(app.state(), "dos.srt".into(), second.into());
        (report.sentences_added, sentences_list(app.state()))
    });
    assert_eq!(added, 1, "{listed:?}");
    assert_eq!(listed.len(), 3);
    assert_eq!(listed[0].es, "¿Dónde está la casa de tu madre?");
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
        round_submit(app.state(), start.session_id, match_round_id(&start.rounds[index]), result);

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
            word_mark_known(app.state(), w.word_id);
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
        srt_import(app.state(), "test.srt".into(), srt.into());
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
        sentence_add_to_deck(app.state(), first.id);
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
        settings_set(app.state(), s);
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
