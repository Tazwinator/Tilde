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
