//! Smoke tests against the real bundled content DB (built by tilde-pipeline).

use tilde_lib::content::ContentDb;
use tilde_lib::db;
use tilde_lib::deck;
use tilde_lib::session;
use tilde_lib::tts::Tts;

fn content_db() -> ContentDb {
    let p = std::path::Path::new("resources/content.db");
    let p = if p.exists() {
        p.to_path_buf()
    } else {
        std::path::PathBuf::from("../resources/content.db")
    };
    ContentDb::open(&p).unwrap()
}

/// A fresh user DB per test: tests run in parallel and must not share progress.
fn user_db(test: &str) -> rusqlite::Connection {
    let path = std::env::temp_dir().join(format!("tilde_test_{}_{test}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    db::open(&path).unwrap()
}

#[test]
fn content_db_has_data() {
    let c = content_db();
    assert!(c.search("hablar", 10).iter().any(|w| w.lemma == "hablar"));
    let words = c.new_candidates(1, 20, &Default::default());
    assert!(!words.is_empty());
    assert!(c.verbs(50).len() >= 10);
}

#[test]
fn generates_every_session_kind() {
    let content = content_db();
    let user = user_db("kinds");
    let tts = Tts::off(&std::env::temp_dir());
    // something due, so ReviewOnly has a card to show
    deck::introduce(&user, content.new_candidates(1, 1, &Default::default())[0].word_id);
    for kind in [
        tilde_core::types::SessionKind::Quick,
        tilde_core::types::SessionKind::Standard,
        tilde_core::types::SessionKind::Deep,
        tilde_core::types::SessionKind::Sidecar,
        tilde_core::types::SessionKind::ReviewOnly,
    ] {
        let g = session::generate(kind, &content, &user, &tts, "en", 1);
        assert!(!g.rounds.is_empty(), "{kind:?} generated no rounds");
        assert_eq!(g.rounds.len(), g.round_types.len());
    }
}

#[test]
fn full_session_lifecycle() {
    let content = content_db();
    let user = user_db("lifecycle");
    let tts = Tts::off(&std::env::temp_dir());

    // placement
    let items = session::placement_items(&content);
    assert!(!items.is_empty());
    let answers: Vec<tilde_core::types::PlacementAnswer> = items
        .iter()
        .take(4)
        .map(|i| tilde_core::types::PlacementAnswer {
            word_id: i.word_id,
            correct: true,
        })
        .collect();
    let result = session::placement_result(&user, &content, &answers);
    assert!(result.frontier_rank > 0);
    assert!(result.words_marked_known > 0);

    // session + grading
    let g = session::generate(
        tilde_core::types::SessionKind::Standard,
        &content,
        &user,
        &tts,
        "en",
        result.frontier_rank,
    );
    let mut xp = 0i64;
    let mut combo = 0i32;
    let mut best = 0i32;
    for (i, (round, ty)) in g.rounds.iter().zip(g.round_types.iter()).enumerate() {
        if *ty == "new_word" {
            continue;
        }
        let feedback = session::score_round(&user, ty, true, Some(3), &mut combo, &mut best, &mut xp);
        assert!(feedback.xp_gained > 0);
        assert!(
            session::correct_answer_of(round).is_some() || matches!(*ty, "match" | "review_card"),
            "{ty} missing correct answer",
        );
        user.execute(
            "INSERT INTO round_log(ts, session_id, round_type, correct) VALUES (?1, 1, ?2, 1)",
            rusqlite::params![deck::now() as i64, ty],
        )
        .unwrap();
        let _ = i;
    }
    assert!(xp > 0);
    let (_, _, known) = deck::counts(&user);
    assert!(known > 0, "grading should create cards");
}

#[test]
fn tts_fallback_is_safe() {
    let tts = Tts::discover(&std::env::temp_dir());
    // must not panic; may return None if no engine
    let _ = tts.speak("hola");
}
