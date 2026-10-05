//! FSRS spaced-repetition scheduling on top of the user cards table.

use fsrs::{FSRS, MemoryState, NextStates};
use rusqlite::Connection;

pub const DESIRED_RETENTION: f32 = 0.9;

pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[derive(Debug, Clone)]
pub struct Card {
    #[allow(dead_code)]
    pub word_id: i64,
    pub stability: f32,
    pub difficulty: f32,
    #[allow(dead_code)]
    pub due_ts: f64,
    pub last_review_ts: Option<f64>,
    pub reps: i64,
    pub lapses: i64,
    /// 0 = new, 1 = learning, 2 = review
    #[allow(dead_code)]
    pub state: i64,
}

impl Card {
    fn memory_state(&self) -> Option<MemoryState> {
        if self.reps == 0 {
            None
        } else {
            Some(MemoryState {
                stability: self.stability,
                difficulty: self.difficulty,
            })
        }
    }
}

fn next_states(card: &Card) -> NextStates {
    let sched = FSRS::default();
    let elapsed = card
        .last_review_ts
        .map(|t| ((now() - t) / 86400.0).max(0.0))
        .unwrap_or(0.0) as u32;
    sched
        .next_states(card.memory_state(), DESIRED_RETENTION, elapsed)
        .expect("fsrs next_states")
}

pub fn grade(conn: &Connection, word_id: i64, quality: i32) {
    let card = load(conn, word_id).unwrap_or(Card {
        word_id,
        stability: 0.0,
        difficulty: 5.0,
        due_ts: 0.0,
        last_review_ts: None,
        reps: 0,
        lapses: 0,
        state: 0,
    });
    let ns = next_states(&card);
    let chosen = match quality {
        1 => ns.again,
        2 => ns.hard,
        4 => ns.easy,
        _ => ns.good,
    };
    // "again" keeps the card in learning; good+ moves it toward review
    let new_state = if quality == 1 && card.reps < 2 {
        1
    } else {
        2
    };
    let interval_days = chosen.interval.max(0.007); // ~10 min minimum
    let due = now() + f64::from(interval_days) * 86400.0;
    conn.execute(
        "INSERT INTO cards(word_id, stability, difficulty, due_ts, last_review_ts, reps, lapses, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(word_id) DO UPDATE SET
           stability = excluded.stability,
           difficulty = excluded.difficulty,
           due_ts = excluded.due_ts,
           last_review_ts = excluded.last_review_ts,
           reps = excluded.reps,
           lapses = excluded.lapses,
           state = excluded.state",
        rusqlite::params![
            word_id,
            chosen.memory.stability,
            chosen.memory.difficulty,
            due,
            now(),
            card.reps + 1,
            card.lapses + i64::from(quality == 1),
            new_state,
        ],
    )
    .ok();
    conn.execute(
        "INSERT INTO reviews(word_id, ts, grade) VALUES (?1, ?2, ?3)",
        rusqlite::params![word_id, now() as i64, quality],
    )
    .ok();
}

pub fn mark_known(conn: &Connection, word_id: i64) {
    conn.execute(
        "INSERT INTO cards(word_id, stability, difficulty, due_ts, last_review_ts, reps, lapses, state)
         VALUES (?1, 120.0, 4.5, ?2, ?3, 3, 0, 2)
         ON CONFLICT(word_id) DO UPDATE SET
           stability = 120.0, due_ts = excluded.due_ts, state = 2",
        rusqlite::params![word_id, now() + 180.0 * 86400.0, now()],
    )
    .ok();
}

pub fn reset(conn: &Connection, word_id: i64) {
    conn.execute("DELETE FROM cards WHERE word_id = ?1", [word_id])
        .ok();
}

pub fn introduce(conn: &Connection, word_id: i64) {
    conn.execute(
        "INSERT OR IGNORE INTO cards(word_id, stability, difficulty, due_ts, last_review_ts, reps, lapses, state)
         VALUES (?1, 0.0, 5.0, ?2, NULL, 0, 0, 1)",
        rusqlite::params![word_id, now()],
    )
    .ok();
}

pub fn load(conn: &Connection, word_id: i64) -> Option<Card> {
    conn.query_row(
        "SELECT word_id, stability, difficulty, due_ts, last_review_ts, reps, lapses, state
         FROM cards WHERE word_id = ?1",
        [word_id],
        |r| {
            Ok(Card {
                word_id: r.get(0)?,
                stability: r.get(1)?,
                difficulty: r.get(2)?,
                due_ts: r.get(3)?,
                last_review_ts: r.get(4)?,
                reps: r.get(5)?,
                lapses: r.get(6)?,
                state: r.get(7)?,
            })
        },
    )
    .ok()
}

pub fn due_card_ids(conn: &Connection, limit: i64) -> Vec<i64> {
    let mut stmt = conn
        .prepare(
            "SELECT word_id FROM cards WHERE due_ts <= ?1 AND state > 0
             ORDER BY due_ts LIMIT ?2",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![now(), limit], |r| r.get(0))
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

pub fn learning_card_ids(conn: &Connection, limit: i64) -> Vec<i64> {
    let mut stmt = conn
        .prepare(
            "SELECT word_id FROM cards WHERE state > 0 ORDER BY due_ts LIMIT ?1",
        )
        .unwrap();
    stmt.query_map([limit], |r| r.get(0))
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

/// Every word the player has a card for, whatever its state.
pub fn card_ids(conn: &Connection) -> std::collections::HashSet<i64> {
    let mut stmt = conn.prepare("SELECT word_id FROM cards").unwrap();
    stmt.query_map([], |r| r.get(0))
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

pub fn counts(conn: &Connection) -> (i64, i64, i64) {
    let now_ts = now();
    let due: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cards WHERE due_ts <= ?1 AND state > 0",
            [now_ts],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let learning: i64 = conn
        .query_row("SELECT COUNT(*) FROM cards WHERE state > 0", [], |r| r.get(0))
        .unwrap_or(0);
    let known: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM cards WHERE state = 2 AND stability >= 21",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    (due, learning, known)
}
