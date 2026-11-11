pub mod backup;
pub mod content;
pub mod db;
pub mod deck;
pub mod session;
pub mod tts;

#[cfg(test)]
mod tests;

use content::ContentDb;
use rusqlite::Connection;
use session::{Session};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tilde_core::types::{
    BackupFolder, BackupInfo, BackupProblem, Badge, Profile, Round, RoundFeedback, RoundResult, SessionKind, SessionStart, SessionSummary, Settings, WordHit,
    TtsInfo, WordDetail, ExSentence, CardStateInfo, ImportReport, MinedSentence,
    PlacementAnswer, PlacementItem, PlacementResult, StatsData, SeriesPoint,
};

// Every command is `#[tauri::command(async)]`: it runs on Tauri's thread
// pool, so a slow one (TTS, a big subtitle file, a backup) never freezes the
// window. Locks are always taken in the order sessions → user → content.
// `backup_error` is a leaf: held for a moment, never while taking another.
pub struct AppState {
    pub content: Mutex<ContentDb>,
    pub user: Mutex<Connection>,
    pub tts: Arc<tts::Tts>,
    pub app_dir: PathBuf,
    pub sessions: Mutex<HashMap<i64, Session>>,
    pub next_session: AtomicI64,
    /// Why the latest automatic backup failed, until a write into the backup
    /// folder works again. In memory only: the next session end sets it anew.
    pub backup_error: Mutex<Option<BackupProblem>>,
}

impl AppState {
    pub fn new(content: ContentDb, user: Connection, tts: tts::Tts, app_dir: PathBuf) -> AppState {
        AppState {
            content: Mutex::new(content),
            user: Mutex::new(user),
            tts: Arc::new(tts),
            app_dir,
            sessions: Mutex::new(HashMap::new()),
            next_session: AtomicI64::new(1),
            backup_error: Mutex::new(None),
        }
    }

    /// A backup into the current folder worked, so whatever stopped the last
    /// automatic one is over.
    fn backup_worked(&self) {
        *self.backup_error.locked() = None;
    }

    /// An automatic backup failed. Nobody asked for it, so nobody is waiting
    /// for an answer: keep the reason for Settings to show.
    fn backup_failed(&self, message: String) {
        eprintln!("automatic backup failed: {message}");
        *self.backup_error.locked() = Some(BackupProblem { message, at: deck::now() as i64 });
    }

    fn backup_folder(&self) -> BackupFolder {
        let mut folder = backup::describe(&self.app_dir);
        folder.last_error = self.backup_error.locked().clone();
        folder
    }
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

fn total_xp(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COALESCE(SUM(xp), 0) FROM events",
        [],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command(async)]
fn profile_get(state: tauri::State<AppState>) -> Profile {
    let conn = state.user.locked();
    let content = state.content.locked();
    let (reviews_due, learning, known) = deck::counts(&conn, content.word_ids());
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
    // what a session started now would introduce, capped by what's left to learn
    let new_words = {
        let kinds = [SessionKind::Quick, SessionKind::Standard, SessionKind::Deep, SessionKind::ReviewOnly];
        let planned = |k: &SessionKind| session::planned_new_words(*k, learning as usize);
        let most = kinds.iter().map(planned).max().unwrap_or(0);
        let left = content.new_candidates(most, &deck::card_ids(&conn)).len();
        kinds
            .iter()
            .map(|k| {
                let key = serde_json::to_value(k).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
                (key, planned(k).min(left) as i64)
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
        cefr_estimate: session::cefr_estimate(known).to_string(),
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
    let now = deck::now();
    let session = Session {
        kind,
        rounds: generated.rounds.clone(),
        round_types: generated.round_types,
        combo: 0,
        best_combo: 0,
        xp: 0,
        new_words: Vec::new(),
        active_ms: 0,
        last_active: now,
        correct: 0,
        total_graded: 0,
        event_id: None,
        answered: HashSet::new(),
    };
    let mut sessions = state.sessions.locked();
    // A session whose window was closed on it is never finished, and would
    // sit here until Tilde quits. There is one window, so one session being
    // played at a time; what it earned is already in its `events` row.
    sessions.retain(|_, s| now - s.last_active < session::STALE_S);
    sessions.insert(id, session);
    drop(sessions);
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
) -> Result<RoundFeedback, String> {
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
        return Ok(neutral(0));
    };
    let index = match session.rounds.get(result.round_index) {
        Some(r) if match_round_id(r) == round_id => Some(result.round_index),
        _ => session.rounds.iter().position(|r| match_round_id(r) == round_id),
    };
    // A double Enter or a late click must not grade the same round twice.
    let Some(index) = index.filter(|i| !session.answered.contains(i)) else {
        return Ok(neutral(session.combo));
    };
    // whether or not the answer can be saved, the player is here
    session.last_active = deck::now();
    let before = session.mark();
    let mut user = state.user.locked();
    let content = state.content.locked();
    let graded = db::in_transaction(&mut user, |tx| {
        grade_round(tx, content.word_ids(), session, session_id, index, &result)
    });
    match graded {
        // Answered only now that it is saved...
        Ok(feedback) => {
            session.answered.insert(index);
            Ok(feedback)
        }
        // ...and if it wasn't, nothing of it stays in memory either, so the
        // player can answer the round again and have it count.
        Err(e) => {
            session.rewind(before);
            Err(db::user_error(&e))
        }
    }
}

/// Grades one round: schedules its cards, scores it and logs it. Every write
/// goes through `tx`, the caller's transaction, so a round is saved whole or
/// not at all; on an error `session` may have changed part-way, and the
/// caller rewinds it.
fn grade_round(
    tx: &Connection,
    live: &HashSet<i64>,
    session: &mut Session,
    session_id: i64,
    index: usize,
    result: &RoundResult,
) -> rusqlite::Result<RoundFeedback> {
    let round = session.rounds[index].clone();
    let round_type = session.round_types.get(index).copied().unwrap_or("choice");
    let correct = result.correct;
    let quality = result.quality;

    session.active_ms += result.duration_ms.clamp(0, session::ROUND_MS_CAP);
    if round_type != "new_word" {
        session.total_graded += 1;
        if correct {
            session.correct += 1;
        }
    }

    // schedule the card
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
        deck::grade(tx, wid, grade)?;
    }

    // a new word joins the deck once its intro has actually been seen
    if let Round::NewWord { word, .. } = &round {
        if quality == Some(4) {
            // "Ya me la sé": known, and not one of the session's new words
            deck::mark_known(tx, word.word_id)?;
        } else {
            deck::introduce(tx, word.word_id)?;
            session.new_words.push(word.lemma.clone());
        }
    }

    // match rounds: each pair on its own, so one slip doesn't fail all four
    if let Round::Match { pairs, .. } = &round {
        for p in pairs {
            let missed = result.missed_word_ids.contains(&p.word_id);
            deck::grade(tx, p.word_id, if missed { 1 } else { 3 })?;
        }
    }

    let xp_before = total_xp(tx);
    let outcome = session::score_round(
        tx,
        round_type,
        correct,
        quality,
        &mut session.combo,
        &mut session.best_combo,
        &mut session.xp,
    )?;
    save_session_event(tx, live, session)?;
    tx.execute(
        "INSERT INTO round_log(ts, session_id, round_type, correct) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![deck::now() as i64, session_id, round_type, i64::from(correct)],
    )?;
    Ok(RoundFeedback {
        correct_answer: session::correct_answer_of(&round),
        xp_gained: outcome.xp_gained,
        combo: outcome.combo,
        level_up: session::level_up(xp_before, total_xp(tx)),
        new_badges: outcome.new_badges,
    })
}

/// Writes the session's running totals to its `events` row, creating the row
/// on the first answer, so XP and minutes count even if the session is never
/// finished (the app closed, or the player just walked away).
fn save_session_event(conn: &Connection, live: &HashSet<i64>, session: &mut Session) -> rusqlite::Result<()> {
    let kind_label = match session.kind {
        SessionKind::Sidecar => "sidecar",
        _ => "session",
    };
    let meta = serde_json::to_string(&EventMeta {
        words_known: deck::counts(conn, live).2,
        kind_label: kind_label.to_string(),
    })
    .unwrap_or_default();
    let now = deck::now();
    let duration_s = session.active_ms / 1000;
    match session.event_id {
        Some(id) => {
            conn.execute(
                "UPDATE events SET ts = ?1, xp = ?2, duration_s = ?3, meta = ?4 WHERE id = ?5",
                rusqlite::params![now as i64, session.xp, duration_s, meta, id],
            )?;
        }
        None => {
            conn.execute(
                "INSERT INTO events(ts, kind, xp, duration_s, meta) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![now as i64, kind_label, session.xp, duration_s, meta],
            )?;
            session.event_id = Some(conn.last_insert_rowid());
        }
    }
    Ok(())
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
    // A session has its row in `events` from the first round the player got
    // through, an intro card included; one without was opened and left, and
    // has nothing to save or back up.
    let played = session.event_id.is_some();
    let words_learned = {
        let content = state.content.locked();
        if played {
            // Nothing new rides on this: the session's XP and length were
            // saved round by round. Not worth failing the summary for.
            if let Err(e) = save_session_event(&user, content.word_ids(), &mut session) {
                eprintln!("could not update the session's row: {e}");
            }
        }
        deck::learned_count(&user, content.word_ids())
    };

    // badge checks. No transaction here: a badge that can't be written just
    // isn't earned yet, and is checked for again when the next session ends.
    let award = |id: &str| session::award_badge(&user, id).unwrap_or(false);
    let mut badges = Vec::new();
    // Here and not on the first answer: the badge is for finishing, and this
    // is where the summary can show it. It takes an answer, though: new
    // words looked at and nothing more is a summary of no rounds.
    if session.total_graded > 0 && award("first-session") {
        badges.push(session::find_badge("first-session"));
    }
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
        (words_learned >= 50, "words-50"),
        (words_learned >= 200, "words-200"),
        (words_learned >= 500, "words-500"),
        (words_learned >= 1000, "words-1000"),
        (minutes_total >= 60, "minutes-60"),
        (minutes_total >= 300, "minutes-300"),
        (minutes_total >= 900, "minutes-900"),
        (minutes_total >= 1800, "minutes-1800"),
    ] {
        if cond && award(id) {
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
    if days_since >= 7 && award("return-trip") {
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
    if conjugation_correct >= 100 && award("conjugation-master") {
        badges.push(session::find_badge("conjugation-master"));
    }
    let listening_correct: i64 = user
        .query_row(
            "SELECT COUNT(*) FROM round_log WHERE round_type IN ('listen','listen_type') AND correct=1",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if listening_correct >= 200 && award("listener") {
        badges.push(session::find_badge("listener"));
    }
    let sidecar_minutes: i64 = user
        .query_row(
            "SELECT COALESCE(SUM(duration_s),0)/60 FROM events WHERE kind='sidecar'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if sidecar_minutes >= 60 && award("sidecar-hour") {
        badges.push(session::find_badge("sidecar-hour"));
    }
    // the session's XP is already in the total; compare with where it started
    let total = total_xp(&user);
    let level_up = session::level_up(total - session.xp, total);
    let summary = session::session_summary(&user, &session, level_up, badges);
    if played {
        match backup::write(&user, &backup::folder(&state.app_dir), backup::Kind::Auto) {
            Ok(_) => state.backup_worked(),
            Err(e) => state.backup_failed(e),
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
) -> Result<PlacementResult, String> {
    let mut conn = state.user.locked();
    let content = state.content.locked();
    db::in_transaction(&mut conn, |tx| {
        // Placement happens once. Asked again (a second click, a stale page)
        // it answers as it did then and marks nothing more.
        let done = db::get_setting(tx, "placement").and_then(|v| serde_json::from_str(&v).ok());
        if let Some(result) = done {
            return Ok(result);
        }
        let result = session::placement_result(tx, &content, &answers)?;
        db::set_setting(tx, "placement", &serde_json::to_string(&result).unwrap())?;
        Ok(result)
    })
    .map_err(|e| db::user_error(&e))
}

// --- words ---

/// Whether the dictionary has this word. Commands that are handed a word id
/// do nothing for one it hasn't. The content lock is let go before this
/// returns: a caller that goes on to lock `user` must not hold it (the order
/// is user, then content).
fn in_content(state: &AppState, word_id: i64) -> bool {
    state.content.locked().word_ids().contains(&word_id)
}

fn hit(conn: &Connection, w: &tilde_core::types::WordCard) -> WordHit {
    let in_deck = conn
        .query_row("SELECT EXISTS(SELECT 1 FROM cards WHERE word_id = ?1)", [w.word_id], |r| r.get(0))
        .unwrap_or(false);
    session::to_hit(w, deck::is_known(conn, w.word_id), in_deck)
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
        .locked()
        .sentences_for_word(word_id, 6)
        .into_iter()
        .map(|s| ExSentence {
            id: s.id,
            es: s.es,
            en: s.en,
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
fn word_mark_known(state: tauri::State<AppState>, word_id: i64) -> Result<(), String> {
    if !in_content(&state, word_id) {
        return Ok(());
    }
    let conn = state.user.locked();
    deck::mark_known(&conn, word_id).map_err(|e| db::user_error(&e))
}

#[tauri::command(async)]
fn word_reset(state: tauri::State<AppState>, word_id: i64) -> Result<(), String> {
    // The card of a word that left the dictionary is dormant, not rubbish: it
    // works again if the word comes back, so it isn't deleted by id either.
    if !in_content(&state, word_id) {
        return Ok(());
    }
    let conn = state.user.locked();
    deck::reset(&conn, word_id).map_err(|e| db::user_error(&e))
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
fn srt_import(state: tauri::State<AppState>, title: String, text: String) -> Result<ImportReport, String> {
    let lines = tilde_core::srt::parse_subtitles(&text);
    let sentences = tilde_core::srt::lines_to_sentences(&lines);

    let matcher = state.content.locked().form_index();
    let mut conn = state.user.locked();

    let mut added = 0i64;
    let mut word_freq: HashMap<i64, i64> = HashMap::new();
    // sentences already mined, from this file or any earlier import, however
    // they were punctuated there
    let mut seen: HashSet<String> = conn
        .prepare("SELECT es FROM mined")
        .and_then(|mut stmt| {
            stmt.query_map([], |r| r.get::<_, String>(0))
                .map(|rows| rows.filter_map(|r| r.ok()).map(|es| tilde_core::srt::sentence_key(&es)).collect())
        })
        .unwrap_or_default();
    // The whole file or none of it: a write that fails part-way must not
    // leave some sentences in, or a sentence without its words.
    db::in_transaction(&mut conn, |tx| {
        for s in &sentences {
            let m = tilde_core::srt::match_sentence(&s.text, &matcher);
            if m.coverage < 0.35 {
                continue;
            }
            if !seen.insert(tilde_core::srt::sentence_key(&s.text)) {
                continue;
            }
            tx.execute(
                "INSERT INTO mined(es, en, source, created_at, in_deck) VALUES (?1, NULL, ?2, ?3, 0)",
                rusqlite::params![s.text, title, deck::now() as i64],
            )?;
            let mid = tx.last_insert_rowid();
            added += 1;
            for mw in &m.words {
                for wid in &mw.word_ids {
                    *word_freq.entry(*wid).or_insert(0) += 1;
                    tx.execute(
                        "INSERT OR IGNORE INTO mined_words(mined_id, word_id) VALUES (?1, ?2)",
                        rusqlite::params![mid, wid],
                    )?;
                }
            }
        }

        // badges earned here are surfaced on the next profile refresh
        if added > 0 {
            session::award_badge(tx, "first-mined")?;
        }
        let mined_count: i64 = tx
            .query_row("SELECT COUNT(*) FROM mined", [], |r| r.get(0))
            .unwrap_or(0);
        if mined_count >= 50 {
            session::award_badge(tx, "mined-50")?;
        }
        Ok(())
    })
    .map_err(|e| db::user_error(&e))?;
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

    Ok(ImportReport {
        file_title: title,
        sentences_found: sentences.len() as i64,
        sentences_added: added,
        words_matched: top.len() as i64,
        top_words,
    })
}

/// One page of the mined sentences, newest first: the ones below `before_id`
/// (the last id of the page before), or the newest when there is none. Paged
/// by id, not by offset, so an import between two pages shifts nothing.
fn mined_page(conn: &Connection, before_id: Option<i64>, limit: Option<i64>) -> tilde_core::types::SentencePage {
    let limit = limit.unwrap_or(100).clamp(1, 500);
    let mut stmt = conn
        .prepare(
            "SELECT id, es, en, source, created_at, in_deck FROM mined
             WHERE (?1 IS NULL OR id < ?1) ORDER BY id DESC LIMIT ?2",
        )
        .unwrap();
    let sentences = stmt
        .query_map(rusqlite::params![before_id, limit], |r| {
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
        .unwrap_or_default();
    let total = conn
        .query_row("SELECT COUNT(*) FROM mined", [], |r| r.get(0))
        .unwrap_or(0);
    tilde_core::types::SentencePage { sentences, total }
}

#[tauri::command(async)]
fn sentences_list(
    state: tauri::State<AppState>,
    before_id: Option<i64>,
    limit: Option<i64>,
) -> tilde_core::types::SentencePage {
    mined_page(&state.user.locked(), before_id, limit)
}

#[tauri::command(async)]
fn sentence_add_to_deck(state: tauri::State<AppState>, id: i64) -> Result<(), String> {
    // The sentence is matched again, as an import would match it today,
    // rather than read back from `mined_words`. What was stored at import
    // may be from an older Tilde, which linked a form to every word it could
    // belong to ("niños" to "niña" too), or from an older dictionary with
    // words that have since left it; neither should become a card now.
    //
    // Matching takes the form index, and building it keeps the dictionary
    // from every other command for a moment. So the sentence is read first:
    // one that is no longer there has no words to add. Each lock is let go
    // before the next is taken.
    let stored = state
        .user
        .locked()
        .query_row("SELECT es FROM mined WHERE id = ?1", [id], |r| r.get::<_, String>(0));
    let es = match stored {
        Ok(es) => es,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(()),
        Err(e) => return Err(db::user_error(&e)),
    };
    let matcher = state.content.locked().form_index();
    let said: Vec<i64> = tilde_core::srt::match_sentence(&es, &matcher)
        .words
        .into_iter()
        .flat_map(|w| w.word_ids)
        .collect();
    let mut conn = state.user.locked();
    db::in_transaction(&mut conn, |tx| add_sentence_words(tx, id, &es, &said))
        .map_err(|e| db::user_error(&e))
}

/// Marks mined sentence `id` as added to the deck and gives each of
/// `word_ids`, the words matched in its text `es`, a card. For use in a
/// transaction. The progress was not locked while the text was matched: if
/// the sentence under this id no longer reads `es` (it was deleted, or a
/// backup with other sentences was restored), nothing is done.
fn add_sentence_words(tx: &Connection, id: i64, es: &str, word_ids: &[i64]) -> rusqlite::Result<()> {
    if tx.execute("UPDATE mined SET in_deck = 1 WHERE id = ?1 AND es = ?2", rusqlite::params![id, es])? == 0 {
        return Ok(());
    }
    for wid in word_ids {
        deck::introduce(tx, *wid)?;
    }
    Ok(())
}

// --- stats ---

/// One point per local calendar day (the day's last count), labelled
/// YYYY-MM-DD so points from different years never share a label. `rows` are
/// `(ts, words known)`, oldest first.
fn words_by_day<Tz: chrono::TimeZone>(rows: &[(i64, i64)], tz: &Tz) -> Vec<SeriesPoint> {
    let mut days: Vec<(chrono::NaiveDate, i64)> = Vec::new();
    for &(ts, words) in rows {
        let Some(day) = tz.timestamp_opt(ts, 0).single().map(|t| t.date_naive()) else {
            continue;
        };
        match days.last_mut() {
            // `<=`, not `==`: where the clocks are put back across midnight,
            // a later moment has the earlier date. It joins the day already
            // open, so the dates only ever go forward.
            Some(last) if day <= last.0 => last.1 = words,
            _ => days.push((day, words)),
        }
    }
    days.into_iter()
        .map(|(day, words)| SeriesPoint {
            label: day.format("%Y-%m-%d").to_string(),
            value: words as f64,
        })
        .collect()
}

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
        let label = if w == 0 { "ahora".to_string() } else { format!("-{w} sem") };
        minutes_per_week.push(SeriesPoint {
            label: label.clone(),
            value: mins.round(),
        });
        xp_per_week.push(SeriesPoint {
            label,
            value: xp as f64,
        });
    }

    let cumulative: Vec<(i64, i64)> = {
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
    let cumulative_words = words_by_day(&cumulative, &chrono::Local);

    // NULL with no graded round yet: that is "nothing to say", not 0%
    let accuracy_all_time: Option<f32> = conn
        .query_row(
            "SELECT AVG(correct) FROM round_log WHERE round_type != 'new_word'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(None);
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

/// Returns the settings as stored, which is what the form should show.
#[tauri::command(async)]
fn settings_set(state: tauri::State<AppState>, settings: Settings) -> Result<Settings, String> {
    let mut conn = state.user.locked();
    db::in_transaction(&mut conn, |tx| {
        db::set_setting(tx, "definition_lang", &settings.definition_lang)?;
        db::set_setting(tx, "session_length", &settings.session_length)?;
        db::set_setting(tx, "sound_enabled", if settings.sound_enabled { "1" } else { "0" })
    })
    .map_err(|e| db::user_error(&e))?;
    Ok(self::settings(&conn))
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

fn user_db_path(app_dir: &Path) -> PathBuf {
    app_dir.join(db::FILE_NAME)
}

#[tauri::command(async)]
fn backup_folder(state: tauri::State<AppState>) -> BackupFolder {
    state.backup_folder()
}

/// `dir: None` goes back to the default folder inside the app's data dir.
#[tauri::command(async)]
fn backup_folder_set(state: tauri::State<AppState>, dir: Option<String>) -> Result<BackupFolder, String> {
    let dir = backup::set_folder(&state.app_dir, dir.as_deref())?;
    // whatever went wrong was about the old folder
    state.backup_worked();
    // Start the new folder off with today's progress rather than waiting for
    // the next session, so a freshly synced folder is useful straight away.
    let conn = state.user.locked();
    if let Err(e) = backup::write(&conn, &dir, backup::Kind::Auto) {
        state.backup_failed(e);
    }
    drop(conn);
    Ok(state.backup_folder())
}

#[tauri::command(async)]
fn backup_export(state: tauri::State<AppState>) -> Result<String, String> {
    let conn = state.user.locked();
    let path = backup::write(&conn, &backup::folder(&state.app_dir), backup::Kind::Manual)?;
    state.backup_worked();
    Ok(path.to_string_lossy().into_owned())
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
    // the safety copy went into the backup folder
    state.backup_worked();
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
    state.backup_worked();
    // One transaction, children before parents (mined_words references mined),
    // so a failure leaves everything as it was rather than half-deleted.
    db::in_transaction(&mut conn, |tx| {
        for table in ["mined_words", "mined", "cards", "reviews", "round_log", "events", "badges", "settings"] {
            tx.execute(&format!("DELETE FROM {table}"), [])?;
        }
        Ok(())
    })
    .map_err(|e| db::user_error(&e))
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

/// The progress database as startup leaves it.
struct Opened {
    conn: Connection,
    /// Where the damaged database went, if there was one.
    set_aside: Option<PathBuf>,
    /// The backup that then took its place, if one could.
    recovered: Option<BackupInfo>,
}

/// Opens the player's progress. A damaged database is set aside and the
/// newest backup recovered in its place straight away, not left for the
/// player to restore later: every session played on the empty database in
/// between would push an automatic backup of real progress out of the
/// newest `KEEP_AUTO`.
fn open_progress(app_dir: &Path) -> Result<Opened, String> {
    let (mut conn, set_aside) = db::open_or_set_aside(&user_db_path(app_dir))?;
    let recovered = match set_aside {
        Some(_) => backup::recover_newest(&mut conn, &backup::folder(app_dir)),
        None => None,
    };
    Ok(Opened { conn, set_aside, recovered })
}

/// What to tell the player after their progress was set aside at startup.
fn set_aside_message(aside: &Path, recovered: Option<&BackupInfo>) -> String {
    let kept = format!(
        "Tu progreso estaba dañado y no se podía leer. Lo hemos guardado aparte, sin borrar nada:\n{}",
        aside.display()
    );
    match recovered {
        Some(backup) => {
            let when = chrono::DateTime::from_timestamp(backup.created_at, 0)
                .map(|t| t.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M").to_string())
                .unwrap_or_default();
            format!("{kept}\n\nHemos recuperado tu copia de seguridad del {when}. Lo que jugaste después de esa fecha no está en la copia.")
        }
        None => format!("{kept}\n\nEmpiezas con un progreso nuevo; para recuperar el anterior, restaura una copia en Ajustes → Copias de seguridad."),
    }
}

/// Held for as long as this process runs; the OS lets go of it when the
/// process ends, however it ends.
static INSTANCE_LOCK: OnceLock<std::fs::File> = OnceLock::new();

/// Takes this data directory for one running Tilde. `Err` means another one
/// has it. `Ok(None)` means the lock couldn't be tried at all (some network
/// file systems can't): not a reason to refuse to start.
///
/// The single-instance plugin catches a second launch first, but only over
/// the session bus, and says nothing when there is none. Two Tildes on one
/// database must not happen: startup may rename the file, and sweeps
/// temporary files the other one could be writing.
fn lock_instance(app_dir: &Path) -> Result<Option<std::fs::File>, ()> {
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(app_dir.join("tilde.lock"))
    {
        Ok(file) => file,
        Err(e) => {
            eprintln!("could not open the instance lock: {e}");
            return Ok(None);
        }
    };
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Err(()),
        Err(std::fs::TryLockError::Error(e)) => {
            eprintln!("could not lock this instance: {e}");
            Ok(None)
        }
    }
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
    let builder = tauri::Builder::default();
    // Registered first: a second launch stops here, before it opens anything,
    // and brings the first one's window forward instead.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        // No state means the "can't start" dialog is up, with the window
        // hidden behind it on purpose.
        if app.try_state::<AppState>().is_none() {
            return;
        }
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let _ = std::fs::create_dir_all(&app_dir);
            match lock_instance(&app_dir) {
                Ok(Some(lock)) => {
                    let _ = INSTANCE_LOCK.set(lock);
                }
                Ok(None) => {}
                Err(()) => {
                    cannot_start(app, "Tilde ya está abierto. Cierra la otra ventana de Tilde y vuelve a intentarlo.".into());
                    return Ok(());
                }
            }
            let content = match ContentDb::open(&resolve_content_db(app.handle())) {
                Ok(content) => content,
                Err(e) => {
                    cannot_start(app, format!(
                        "Falta el diccionario de Tilde o está dañado. Reinstalar Tilde lo arregla; tu progreso no se toca.\n\n({e})"
                    ));
                    return Ok(());
                }
            };
            let opened = match open_progress(&app_dir) {
                Ok(opened) => opened,
                Err(e) => {
                    cannot_start(app, format!("No se pudo abrir tu progreso.\n\n{e}"));
                    return Ok(());
                }
            };
            if let Some(aside) = &opened.set_aside {
                app.dialog()
                    .message(set_aside_message(aside, opened.recovered.as_ref()))
                    .title("Tilde")
                    .kind(MessageDialogKind::Warning)
                    .show(|_| {});
            }
            let tts = tts::Tts::discover(&app_dir);
            app.manage(AppState::new(content, opened.conn, tts, app_dir.clone()));
            // What a killed run left half-written. In the background: the
            // backup folder may be a slow network or synced one.
            std::thread::spawn(move || {
                let hour = std::time::Duration::from_secs(3600);
                if let Some(hour_ago) = std::time::SystemTime::now().checked_sub(hour) {
                    backup::sweep_stale(&app_dir, &tts::cache_dir(&app_dir), &backup::folder(&app_dir), hour_ago);
                }
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
