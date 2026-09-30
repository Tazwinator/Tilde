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
    let user = db::open(&app_dir.join("tilde_user.db"));
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
        let detail = word_detail(app.state(), hits[0].word_id).expect("word detail");
        assert_eq!(detail.word.word_id, hits[0].word_id);
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
