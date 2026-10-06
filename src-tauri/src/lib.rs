pub mod backup;
pub mod content;
pub mod db;
pub mod deck;
pub mod session;
pub mod tts;

#[cfg(test)]
mod tests;

use base64::Engine;
use content::ContentDb;
use rusqlite::Connection;
use session::{Session};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tilde_core::types::{
    BackupFolder, Badge, Profile, Round, RoundFeedback, RoundResult, SessionKind, SessionStart, SessionSummary, Settings, WordHit,
    TtsInfo, WordDetail, ExSentence, CardStateInfo, ImportReport, MinedSentence,
    PlacementAnswer, PlacementItem, PlacementResult, StatsData, SeriesPoint,
};

// Every command is `#[tauri::command(async)]`: it runs on Tauri's thread
// pool, so a slow one (TTS, a big subtitle file, a backup) never freezes the
// window. Locks are always taken in the order sessions → user → content.
pub struct AppState {
    pub content: Mutex<ContentDb>,
    pub user: Mutex<Connection>,
    pub tts: Arc<tts::Tts>,
    pub app_dir: PathBuf,
    pub sessions: Mutex<HashMap<i64, Session>>,
    pub next_session: AtomicI64,
}

/// `Mutex::lock` that shrugs off poisoning. A command that panics while
/// holding a lock (SQLite rolls back whatever it was doing) mustn't make
/// every later command panic too, leaving the app dead until restarted.
trait Locked<T> {
    fn locked(&self) -> std::sync::MutexGuard<'_, T>;
}

impl<T> Locked<T> for Mutex<T> {
    fn locked(&self) -> std::sync::MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventMeta {
    words_known: i64,
    kind_label: String,
}

// Helpers take the caller's `&Connection` rather than `&AppState`: commands
// already hold the `state.user` lock, and `std::sync::Mutex` deadlocks if the
// same thread locks it again.

fn settings(conn: &Connection) -> Settings {
    Settings {
        definition_lang: db::get_setting(conn, "definition_lang").unwrap_or_else(|| "en".into()),
        session_length: db::get_setting(conn, "session_length").unwrap_or_else(|| "standard".into()),
        sound_enabled: db::get_setting(conn, "sound_enabled").map(|v| v == "1").unwrap_or(true),
        target_lang: "es".into(),
    }
}

fn frontier(conn: &Connection) -> i64 {
    db::get_setting(conn, "frontier_rank")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

fn total_xp(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COALESCE(SUM(xp), 0) FROM events",
        [],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

fn cefr_estimate(words_known: i64) -> &'static str {
    match words_known {
        0..=120 => "A1",
        121..=500 => "A2",
        501..=1200 => "B1",
        1201..=2200 => "B2",
        _ => "C1",
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command(async)]
fn profile_get(state: tauri::State<AppState>) -> Profile {
    let conn = state.user.locked();
    let (reviews_due, learning, known) = deck::counts(&conn);
    let xp = total_xp(&conn);
    let (level, name) = tilde_core::level_for_xp(xp);
    let sessions_total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind IN ('session','sidecar')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let minutes_total: i64 = conn
        .query_row("SELECT COALESCE(SUM(duration_s),0)/60 FROM events", [], |r| r.get(0))
        .unwrap_or(0);
    let days_since: i64 = conn
        .query_row(
            "SELECT CAST((?1 - MAX(ts)) / 86400 AS INTEGER) FROM events WHERE kind IN ('session','sidecar')",
            [deck::now() as i64],
            |r| r.get(0),
        )
        .unwrap_or(-1);
    let badges_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM badges", [], |r| r.get(0))
        .unwrap_or(0);
    let s = settings(&conn);
    // capped by what's actually left to learn from the frontier up
    let new_words = {
        let kinds = [SessionKind::Quick, SessionKind::Standard, SessionKind::Deep, SessionKind::ReviewOnly];
        let most = kinds.iter().map(|k| session::new_word_count(*k)).max().unwrap_or(0);
        let left = state
            .content
            .locked()
            .new_candidates(frontier(&conn), most, &deck::card_ids(&conn))
            .len();
        kinds
            .iter()
            .map(|k| {
                let key = serde_json::to_value(k).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
                (key, session::new_word_count(*k).min(left) as i64)
            })
            .collect()
    };
    Profile {
        xp,
        level,
        level_name: name.to_string(),
        level_progress: (xp % 500) as f32 / 500.0,
        words_known: known,
        words_learning: learning,
        minutes_total,
        cefr_estimate: cefr_estimate(known).to_string(),
        reviews_due,
        new_words_by_kind: new_words,
        days_since_last_session: if sessions_total == 0 { -1 } else { days_since },
        sessions_total,
        badges_count,
        definition_lang: s.definition_lang,
        tts_available: state.tts.available(),
        target_lang: "es".into(),
    }
}

#[tauri::command(async)]
fn session_start(state: tauri::State<AppState>, kind: SessionKind) -> SessionStart {
    let conn = state.user.locked();
    let s = settings(&conn);
    let content = state.content.locked();
    let generated = session::generate(
        kind,
        &content,
        &conn,
        &state.tts,
        &s.definition_lang,
        frontier(&conn),
    );
    drop(conn);
    drop(content);
    // synthesize the audio the rounds will want, in play order, while the
    // player gets going; rounds fetch anything not ready yet on demand
    if !generated.to_speak.is_empty() {
        let tts = Arc::clone(&state.tts);
        let texts = generated.to_speak;
        std::thread::spawn(move || {
            for text in texts {
                tts.speak(&text);
            }
        });
    }
    let id = state.next_session.fetch_add(1, Ordering::SeqCst);
    let session = Session {
        kind,
        rounds: generated.rounds.clone(),
        round_types: generated.round_types,
        results: Vec::new(),
        combo: 0,
        best_combo: 0,
        xp: 0,
        new_words: Vec::new(),
        started_at: deck::now(),
        correct: 0,
        total_graded: 0,
        event_id: None,
        answered: HashSet::new(),
    };
    state.sessions.locked().insert(id, session);
    SessionStart {
        session_id: id,
        kind,
        rounds: generated.rounds,
    }
}

#[tauri::command(async)]
fn round_submit(
    state: tauri::State<AppState>,
    session_id: i64,
    round_id: i64,
    result: RoundResult,
) -> RoundFeedback {
    let neutral = |combo| RoundFeedback {
        correct_answer: None,
        xp_gained: 0,
        combo,
        level_up: None,
        new_badges: Vec::new(),
    };
    let mut sessions = state.sessions.locked();
    // Gone after a restore or reset, or already finished: nothing to grade.
    let Some(session) = sessions.get_mut(&session_id) else {
        return neutral(0);
    };
    let index = match session.rounds.get(result.round_index) {
        Some(r) if match_round_id(r) == round_id => Some(result.round_index),
        _ => session.rounds.iter().position(|r| match_round_id(r) == round_id),
    };
    // A double Enter or a late click must not grade the same round twice.
    let Some(index) = index.filter(|i| session.answered.insert(*i)) else {
        return neutral(session.combo);
    };
    let round = session.rounds[index].clone();
    let round_type = session.round_types.get(index).copied().unwrap_or("choice");
    let correct = result.correct;
    let quality = result.quality;

    session.results.push((correct, quality.unwrap_or(if correct { 3 } else { 1 })));
    session.total_graded += 1;
    if correct {
        session.correct += 1;
    }

    // schedule the card
    let user = state.user.locked();
    if let Some(wid) = match &round {
        Round::Choice { word_id, .. }
        | Round::Listen { word_id, .. }
        | Round::ListenType { word_id, .. }
        | Round::Build { word_id, .. }
        | Round::Cloze { word_id, .. }
        | Round::Conjugation { word_id, .. }
        | Round::ReviewCard { word_id, .. } => Some(*word_id),
        _ => None,
    } {
        let grade = if round_type == "review_card" {
            quality.unwrap_or(3)
        } else if correct {
            3
        } else {
            1
        };
        deck::grade(&user, wid, grade);
    }

    // a new word joins the deck once its intro has actually been seen
    if let Round::NewWord { word, .. } = &round {
        deck::introduce(&user, word.word_id);
        session.new_words.push(word.lemma.clone());
    }

    // match rounds: each pair on its own, so one slip doesn't fail all four
    if let Round::Match { pairs, .. } = &round {
        for p in pairs {
            let missed = result.missed_word_ids.contains(&p.word_id);
            deck::grade(&user, p.word_id, if missed { 1 } else { 3 });
        }
    }

    let xp_before = total_xp(&user);
    let mut outcome = session::score_round(
        &user,
        round_type,
        correct,
        quality,
        &mut session.combo,
        &mut session.best_combo,
        &mut session.xp,
    );
    save_session_event(&user, session);
    user.execute(
        "INSERT INTO round_log(ts, session_id, round_type, correct) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![deck::now() as i64, session_id, round_type, i64::from(correct)],
    )
    .ok();
    if session::award_badge(&user, "first-session") {
        outcome.new_badges.push(session::find_badge("first-session"));
    }
    RoundFeedback {
        correct_answer: session::correct_answer_of(&round),
        xp_gained: outcome.xp_gained,
        combo: outcome.combo,
        level_up: session::level_up(xp_before, total_xp(&user)),
        new_badges: std::mem::take(&mut outcome.new_badges),
    }
}

/// Writes the session's running totals to its `events` row, creating the row
/// on the first answer, so XP and minutes count even if the session is never
/// finished (the app closed, or the player just walked away).
fn save_session_event(conn: &Connection, session: &mut Session) {
    let kind_label = match session.kind {
        SessionKind::Sidecar => "sidecar",
        _ => "session",
    };
    let meta = serde_json::to_string(&EventMeta {
        words_known: deck::counts(conn).2,
        kind_label: kind_label.to_string(),
    })
    .unwrap_or_default();
    let now = deck::now();
    let duration_s = (now - session.started_at).max(0.0) as i64;
    match session.event_id {
        Some(id) => {
            conn.execute(
                "UPDATE events SET ts = ?1, xp = ?2, duration_s = ?3, meta = ?4 WHERE id = ?5",
                rusqlite::params![now as i64, session.xp, duration_s, meta, id],
            )
            .ok();
        }
        None => {
            let inserted = conn.execute(
                "INSERT INTO events(ts, kind, xp, duration_s, meta) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![now as i64, kind_label, session.xp, duration_s, meta],
            );
            if inserted.is_ok() {
                session.event_id = Some(conn.last_insert_rowid());
            }
        }
    }
}

fn match_round_id(r: &Round) -> i64 {
    match r {
        Round::NewWord { id, .. }
        | Round::Choice { id, .. }
        | Round::Match { id, .. }
        | Round::Listen { id, .. }
        | Round::ListenType { id, .. }
        | Round::Build { id, .. }
        | Round::Cloze { id, .. }
        | Round::Conjugation { id, .. }
        | Round::ReviewCard { id, .. } => *id,
    }
}

#[tauri::command(async)]
fn session_finish(state: tauri::State<AppState>, session_id: i64) -> SessionSummary {
    let mut sessions = state.sessions.locked();
    let Some(mut session) = sessions.remove(&session_id) else {
        return SessionSummary {
            xp: 0,
            rounds: 0,
            correct: 0,
            minutes: 0.0,
            new_words: vec![],
            accuracy: 0.0,
            level_up: None,
            new_badges: vec![],
            best_combo: 0,
        };
    };
    let user = state.user.locked();
    if session.event_id.is_some() {
        save_session_event(&user, &mut session);
    }
    let words_known = deck::counts(&user).2;

    // badge checks
    let mut badges = Vec::new();
    let sessions_total: i64 = user
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind IN ('session','sidecar')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let minutes_total: i64 = user
        .query_row("SELECT COALESCE(SUM(duration_s),0)/60 FROM events", [], |r| r.get(0))
        .unwrap_or(0);
    for (cond, id) in [
        (sessions_total >= 10, "ten-sessions"),
        (sessions_total >= 50, "fifty-sessions"),
        (words_known >= 50, "words-50"),
        (words_known >= 200, "words-200"),
        (words_known >= 500, "words-500"),
        (words_known >= 1000, "words-1000"),
        (minutes_total >= 60, "minutes-60"),
        (minutes_total >= 300, "minutes-300"),
        (minutes_total >= 900, "minutes-900"),
        (minutes_total >= 1800, "minutes-1800"),
    ] {
        if cond && session::award_badge(&user, id) {
            badges.push(session::find_badge(id));
        }
    }
    // return-trip: played again after >= 7 days away
    let days_since: i64 = user
        .query_row(
            "SELECT CAST((?1 - MAX(ts)) / 86400 AS INTEGER) FROM events
             WHERE kind IN ('session','sidecar') AND ts < (SELECT MAX(ts) FROM events)",
            [deck::now() as i64],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if days_since >= 7 && session::award_badge(&user, "return-trip") {
        badges.push(session::find_badge("return-trip"));
    }
    // skill counters
    let conjugation_correct: i64 = user
        .query_row(
            "SELECT COUNT(*) FROM round_log WHERE round_type='conjugation' AND correct=1",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if conjugation_correct >= 100 && session::award_badge(&user, "conjugation-master") {
        badges.push(session::find_badge("conjugation-master"));
    }
    let listening_correct: i64 = user
        .query_row(
            "SELECT COUNT(*) FROM round_log WHERE round_type IN ('listen','listen_type') AND correct=1",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if listening_correct >= 200 && session::award_badge(&user, "listener") {
        badges.push(session::find_badge("listener"));
    }
    let sidecar_minutes: i64 = user
        .query_row(
            "SELECT COALESCE(SUM(duration_s),0)/60 FROM events WHERE kind='sidecar'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if sidecar_minutes >= 60 && session::award_badge(&user, "sidecar-hour") {
        badges.push(session::find_badge("sidecar-hour"));
    }
    // the session's XP is already in the total; compare with where it started
    let total = total_xp(&user);
    let level_up = session::level_up(total - session.xp, total);
    let summary = session::session_summary(&user, &session, level_up, badges);
    if session.total_graded > 0 {
        if let Err(e) = backup::write(&user, &backup::folder(&state.app_dir), backup::Kind::Auto) {
            eprintln!("automatic backup failed: {e}");
        }
    }
    summary
}

// --- placement ---

#[tauri::command(async)]
fn placement_status(state: tauri::State<AppState>) -> Option<PlacementResult> {
    let conn = state.user.locked();
    db::get_setting(&conn, "placement")
        .and_then(|v| serde_json::from_str(&v).ok())
}

#[tauri::command(async)]
fn placement_start(state: tauri::State<AppState>) -> Vec<PlacementItem> {
    let content = state.content.locked();
    session::placement_items(&content)
}

#[tauri::command(async)]
fn placement_submit(
    state: tauri::State<AppState>,
    answers: Vec<PlacementAnswer>,
) -> PlacementResult {
    let conn = state.user.locked();
    let result = {
        let content = state.content.locked();
        session::placement_result(&conn, &content, &answers)
    };
    db::set_setting(&conn, "placement", &serde_json::to_string(&result).unwrap());
    if session::award_badge(&conn, "first-session") {
        // placement alone doesn't earn a badge; ignore
    }
    result
}

// --- words ---

fn hit(conn: &Connection, w: &tilde_core::types::WordCard) -> WordHit {
    let known = conn
        .query_row(
            "SELECT state FROM cards WHERE word_id = ?1",
            [w.word_id],
            |r| r.get::<_, i64>(0),
        )
        .map(|s| s >= 2)
        .unwrap_or(false);
    session::to_hit(w, known)
}

#[tauri::command(async)]
fn word_search(state: tauri::State<AppState>, query: String, limit: i64) -> Vec<WordHit> {
    let words = state.content.locked().search(&query, limit);
    let conn = state.user.locked();
    words.iter().map(|w| hit(&conn, w)).collect()
}

#[tauri::command(async)]
fn word_detail(state: tauri::State<AppState>, word_id: i64) -> Option<WordDetail> {
    let w = state.content.locked().word(word_id)?;
    let conn = state.user.locked();
    let card = conn
        .query_row(
            "SELECT state, due_ts, reps, lapses, stability FROM cards WHERE word_id = ?1",
            [word_id],
            |r| {
                Ok(CardStateInfo {
                    state: match r.get::<_, i64>(0)? {
                        1 => "learning".to_string(),
                        2 => "review".to_string(),
                        _ => "new".to_string(),
                    },
                    due_in_days: ((r.get::<_, f64>(1)? - deck::now()) / 86400.0).max(0.0),
                    reps: r.get(2)?,
                    lapses: r.get(3)?,
                    strength: (r.get::<_, f32>(4)? / 100.0).clamp(0.0, 1.0),
                })
            },
        )
        .ok();
    let conjugations = {
        let rows = state.content.locked().conjugations(word_id);
        (!rows.is_empty()).then_some(rows)
    };
    let sentences = state
        .content
        .lock()
        .unwrap()
        .sentences_for_word(word_id, 6)
        .into_iter()
        .map(|(id, es, en)| ExSentence {
            id,
            es,
            en,
            mined: false,
        })
        .collect();
    Some(WordDetail {
        word: hit(&conn, &w),
        audio_available: state.tts.available(),
        card,
        conjugations,
        sentences,
    })
}

#[tauri::command(async)]
fn word_mark_known(state: tauri::State<AppState>, word_id: i64) {
    let conn = state.user.locked();
    deck::mark_known(&conn, word_id);
}

#[tauri::command(async)]
fn word_reset(state: tauri::State<AppState>, word_id: i64) {
    let conn = state.user.locked();
    deck::reset(&conn, word_id);
}

// --- tts ---

#[tauri::command(async)]
fn tts_speak(state: tauri::State<AppState>, text: String) -> Option<String> {
    state.tts.speak(&text)
}

#[tauri::command(async)]
fn tts_info(state: tauri::State<AppState>) -> TtsInfo {
    TtsInfo {
        engine: state.tts.engine().into(),
        voice: state.tts.voice().into(),
    }
}

/// Attribution for the bundled word data (its licenses require it in-app).
#[tauri::command(async)]
fn content_credits(state: tauri::State<AppState>) -> String {
    state.content.locked().meta("sources").unwrap_or_default()
}

// --- sentence mining ---

#[tauri::command(async)]
fn srt_import(state: tauri::State<AppState>, title: String, text: String) -> ImportReport {
    let lines = tilde_core::srt::parse_subtitles(&text);
    let sentences = tilde_core::srt::lines_to_sentences(&lines);

    let matcher = state.content.locked().form_index();
    let conn = state.user.locked();

    let mut added = 0i64;
    let mut word_freq: HashMap<i64, i64> = HashMap::new();
    // sentences already mined, from this file or an earlier import of it
    let mut seen: HashSet<String> = conn
        .prepare("SELECT es FROM mined")
        .and_then(|mut stmt| {
            stmt.query_map([], |r| r.get::<_, String>(0))
                .map(|rows| rows.filter_map(|r| r.ok()).map(|es| es.to_lowercase()).collect())
        })
        .unwrap_or_default();
    for s in &sentences {
        let m = tilde_core::srt::match_sentence(&s.text, &matcher);
        if m.coverage < 0.35 {
            continue;
        }
        let key = s.text.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        conn.execute(
            "INSERT INTO mined(es, en, source, created_at, in_deck) VALUES (?1, NULL, ?2, ?3, 0)",
            rusqlite::params![s.text, title, deck::now() as i64],
        )
        .ok();
        let mid = conn.last_insert_rowid();
        for mw in &m.words {
            for wid in &mw.word_ids {
                *word_freq.entry(*wid).or_insert(0) += 1;
                conn.execute(
                    "INSERT OR IGNORE INTO mined_words(mined_id, word_id) VALUES (?1, ?2)",
                    rusqlite::params![mid, wid],
                )
                .ok();
            }
        }
        added += 1;
    }

    if added > 0 && session::award_badge(&conn, "first-mined") {
        // badge awarded; surfaced on next profile refresh
    }
    let mined_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM mined", [], |r| r.get(0))
        .unwrap_or(0);
    if mined_count >= 50 && session::award_badge(&conn, "mined-50") {
        // badge awarded
    }
    drop(conn);

    let mut top: Vec<(i64, i64)> = word_freq.into_iter().collect();
    top.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    let top_cards: Vec<tilde_core::types::WordCard> = {
        let content = state.content.locked();
        top.iter().take(10).filter_map(|(wid, _)| content.word(*wid)).collect()
    };
    let top_words: Vec<WordHit> = {
        let conn = state.user.locked();
        top_cards.iter().map(|w| hit(&conn, w)).collect()
    };

    ImportReport {
        file_title: title,
        sentences_found: sentences.len() as i64,
        sentences_added: added,
        words_matched: top.len() as i64,
        top_words,
    }
}

#[tauri::command(async)]
fn sentences_list(state: tauri::State<AppState>) -> Vec<MinedSentence> {
    let conn = state.user.locked();
    let mut stmt = conn
        .prepare("SELECT id, es, en, source, created_at, in_deck FROM mined ORDER BY id DESC LIMIT 500")
        .unwrap();
    stmt.query_map([], |r| {
        Ok(MinedSentence {
            id: r.get(0)?,
            es: r.get(1)?,
            en: r.get(2)?,
            source: r.get(3)?,
            created_at: r.get(4)?,
            in_deck: r.get::<_, i64>(5)? == 1,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

#[tauri::command(async)]
fn sentence_add_to_deck(state: tauri::State<AppState>, id: i64) {
    let conn = state.user.locked();
    conn.execute("UPDATE mined SET in_deck = 1 WHERE id = ?1", [id])
        .ok();
    let mut stmt = conn
        .prepare("SELECT word_id FROM mined_words WHERE mined_id = ?1")
        .unwrap();
    let word_ids: Vec<i64> = stmt
        .query_map([id], |r| r.get(0))
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();
    for wid in word_ids {
        deck::introduce(&conn, wid);
    }
}

// --- stats ---

#[tauri::command(async)]
fn stats_get(state: tauri::State<AppState>) -> StatsData {
    let conn = state.user.locked();
    let weeks: Vec<(i64, f64, i64)> = {
        let mut stmt = conn
            .prepare("SELECT ts, duration_s, xp FROM events WHERE kind IN ('session','sidecar')")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as f64, r.get(2)?)))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    };
    let now_ts = deck::now() as i64;
    let mut buckets: HashMap<i64, (f64, i64)> = HashMap::new();
    for (ts, dur, xp) in weeks {
        let days_since = (now_ts - ts) / 86400;
        let week_bucket = days_since / 7;
        let e = buckets.entry(week_bucket).or_insert((0.0, 0));
        e.0 += dur / 60.0;
        e.1 += xp;
    }
    // oldest week first, so the charts read left to right up to "now"
    let mut minutes_per_week = Vec::new();
    let mut xp_per_week = Vec::new();
    for w in (0..8).rev() {
        let (mins, xp) = buckets.get(&w).copied().unwrap_or((0.0, 0));
        let label = if w == 0 { "now".to_string() } else { format!("-{w}w") };
        minutes_per_week.push(SeriesPoint {
            label: label.clone(),
            value: mins.round(),
        });
        xp_per_week.push(SeriesPoint {
            label,
            value: xp as f64,
        });
    }

    let mut cumulative: Vec<(i64, i64)> = {
        let mut stmt = conn
            .prepare("SELECT ts, meta FROM events ORDER BY ts")
            .unwrap();
        stmt.query_map([], |r| {
            let ts: i64 = r.get(0)?;
            let meta: String = r.get(1)?;
            Ok((ts, meta))
        })
        .map(|rows| {
            rows.filter_map(|r| r.ok())
                .filter_map(|(ts, meta)| {
                    serde_json::from_str::<EventMeta>(&meta)
                        .ok()
                        .map(|m| (ts, m.words_known))
                })
                .collect()
        })
        .unwrap_or_default()
    };
    // one point per day: the last count of the day, not the first
    cumulative.reverse();
    cumulative.dedup_by(|later, kept| later.0 / 86400 == kept.0 / 86400);
    cumulative.reverse();
    let cumulative_words: Vec<SeriesPoint> = cumulative
        .iter()
        .map(|(ts, w)| {
            SeriesPoint {
                label: chrono::DateTime::from_timestamp(*ts, 0)
                    .map(|d| d.format("%d/%m").to_string())
                    .unwrap_or_default(),
                value: *w as f64,
            }
        })
        .collect();

    let accuracy_all_time: f32 = conn
        .query_row(
            "SELECT COALESCE(AVG(correct), 0.0) FROM round_log WHERE round_type != 'new_word'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0.0);
    let mut game_mix: Vec<(String, i64)> = {
        let mut stmt = conn
            .prepare(
                "SELECT round_type, COUNT(*) FROM round_log WHERE round_type != 'new_word'
                 GROUP BY round_type ORDER BY COUNT(*) DESC",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    };
    game_mix.truncate(6);
    let game_mix: Vec<SeriesPoint> = game_mix
        .into_iter()
        .map(|(label, value)| SeriesPoint {
            label,
            value: value as f64,
        })
        .collect();

    StatsData {
        minutes_per_week,
        xp_per_week,
        cumulative_words,
        accuracy_all_time,
        game_mix,
    }
}

// --- settings / badges / backup ---

#[tauri::command(async)]
fn settings_get(state: tauri::State<AppState>) -> Settings {
    settings(&state.user.locked())
}

#[tauri::command(async)]
fn settings_set(state: tauri::State<AppState>, settings: Settings) -> Settings {
    {
        let conn = state.user.locked();
        db::set_setting(&conn, "definition_lang", &settings.definition_lang);
        db::set_setting(&conn, "session_length", &settings.session_length);
        db::set_setting(&conn, "sound_enabled", if settings.sound_enabled { "1" } else { "0" });
    }
    settings
}

#[tauri::command(async)]
fn badges_list(state: tauri::State<AppState>) -> Vec<Badge> {
    let conn = state.user.locked();
    let mut earned: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT id, earned_at FROM badges").unwrap();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let id: String = row.get(0).unwrap();
            let at: i64 = row.get(1).unwrap();
            earned.insert(id, at);
        }
    }
    tilde_core::all_badges()
        .into_iter()
        .map(|mut b| {
            if let Some(at) = earned.get(&b.id) {
                b.earned_at = Some(*at);
            }
            b
        })
        .collect()
}

fn user_db_path(app_dir: &std::path::Path) -> PathBuf {
    app_dir.join("tilde_user.db")
}

#[tauri::command(async)]
fn backup_folder(state: tauri::State<AppState>) -> BackupFolder {
    backup::describe(&state.app_dir)
}

/// `dir: None` goes back to the default folder inside the app's data dir.
#[tauri::command(async)]
fn backup_folder_set(state: tauri::State<AppState>, dir: Option<String>) -> Result<BackupFolder, String> {
    let dir = backup::set_folder(&state.app_dir, dir.as_deref())?;
    // Start the new folder off with today's progress rather than waiting for
    // the next session, so a freshly synced folder is useful straight away.
    let conn = state.user.locked();
    if let Err(e) = backup::write(&conn, &dir, backup::Kind::Auto) {
        eprintln!("snapshot into the new backup folder failed: {e}");
    }
    Ok(backup::describe(&state.app_dir))
}

#[tauri::command(async)]
fn backup_export(state: tauri::State<AppState>) -> Result<String, String> {
    let conn = state.user.locked();
    backup::write(&conn, &backup::folder(&state.app_dir), backup::Kind::Manual)
        .map(|p| p.to_string_lossy().into_owned())
}

fn restore_from(state: &AppState, bytes: &[u8]) -> Result<(), String> {
    // Rounds in flight belong to the progress being replaced.
    state.sessions.locked().clear();
    let mut conn = state.user.locked();
    backup::restore(
        &mut conn,
        &user_db_path(&state.app_dir),
        bytes,
        &backup::folder(&state.app_dir),
    )?;
    Ok(())
}

#[tauri::command(async)]
fn backup_import(state: tauri::State<AppState>, bytes: Vec<u8>) -> Result<(), String> {
    restore_from(&state, &bytes)
}

/// Restores one of the backups listed by `backup_folder`.
#[tauri::command(async)]
fn backup_restore(state: tauri::State<AppState>, path: String) -> Result<(), String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("No se pudo leer la copia: {e}"))?;
    restore_from(&state, &bytes)
}

#[tauri::command(async)]
fn progress_reset(state: tauri::State<AppState>) -> Result<(), String> {
    state.sessions.locked().clear();
    let mut conn = state.user.locked();
    backup::write(&conn, &backup::folder(&state.app_dir), backup::Kind::BeforeReset)
        .map_err(|e| format!("No se pudo guardar una copia antes de reiniciar: {e}"))?;
    // One transaction, children before parents (mined_words references mined),
    // so a failure leaves everything as it was rather than half-deleted.
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for table in ["mined_words", "mined", "cards", "reviews", "round_log", "events", "badges", "settings"] {
        tx.execute(&format!("DELETE FROM {table}"), [])
            .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

fn resolve_content_db(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let p = resource_dir.join("resources/content.db");
        if p.exists() {
            return p;
        }
    }
    // dev fallback: cwd is src-tauri during `tauri dev`
    let p = PathBuf::from("resources/content.db");
    if p.exists() {
        return p;
    }
    let p = PathBuf::from("../resources/content.db");
    if p.exists() {
        return p;
    }
    // the bundled location, so the error names where it should have been
    app.path()
        .resource_dir()
        .map(|d| d.join("resources/content.db"))
        .unwrap_or_else(|_| PathBuf::from("resources/content.db"))
}

/// Tilde can't run without its data. With no AppState the webview has
/// nothing to show, so say why in a native dialog and quit when it closes.
fn cannot_start(app: &tauri::App, message: String) {
    eprintln!("{message}");
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    let handle = app.handle().clone();
    app.dialog()
        .message(message)
        .title("Tilde no puede arrancar")
        .kind(MessageDialogKind::Error)
        .show(move |_| handle.exit(1));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let _ = std::fs::create_dir_all(&app_dir);
            let content = match ContentDb::open(&resolve_content_db(app.handle())) {
                Ok(content) => content,
                Err(e) => {
                    cannot_start(app, format!(
                        "Falta el diccionario de Tilde o está dañado. Reinstalar Tilde lo arregla; tu progreso no se toca.\n\n({e})"
                    ));
                    return Ok(());
                }
            };
            let (user, set_aside) = match db::open_or_set_aside(&user_db_path(&app_dir)) {
                Ok(opened) => opened,
                Err(e) => {
                    cannot_start(app, format!("No se pudo abrir tu progreso.\n\n{e}"));
                    return Ok(());
                }
            };
            if let Some(aside) = set_aside {
                app.dialog()
                    .message(format!(
                        "Tu progreso estaba dañado y no se podía leer. Lo hemos guardado aparte, sin borrar nada:\n{}\n\nEmpiezas con un progreso nuevo; para recuperar el anterior, restaura una copia en Ajustes → Copias de seguridad.",
                        aside.display()
                    ))
                    .title("Tilde")
                    .kind(MessageDialogKind::Warning)
                    .show(|_| {});
            }
            let tts = Arc::new(tts::Tts::discover(&app_dir));
            app.manage(AppState {
                content: Mutex::new(content),
                user: Mutex::new(user),
                tts,
                app_dir,
                sessions: Mutex::new(HashMap::new()),
                next_session: AtomicI64::new(1),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            profile_get,
            session_start,
            round_submit,
            session_finish,
            placement_status,
            placement_start,
            placement_submit,
            word_search,
            word_detail,
            word_mark_known,
            word_reset,
            tts_speak,
            tts_info,
            content_credits,
            srt_import,
            sentences_list,
            sentence_add_to_deck,
            stats_get,
            settings_get,
            settings_set,
            badges_list,
            backup_folder,
            backup_folder_set,
            backup_export,
            backup_import,
            backup_restore,
            progress_reset,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Utility kept for future base64 audio plumbing in commands.
pub fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
