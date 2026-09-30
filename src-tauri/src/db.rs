//! User progress database: schema, migrations, settings.

use rusqlite::Connection;

pub fn open(path: &std::path::Path) -> Connection {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(path).expect("open user db");
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS cards(
           word_id INTEGER PRIMARY KEY,
           stability REAL NOT NULL DEFAULT 0,
           difficulty REAL NOT NULL DEFAULT 5,
           due_ts REAL NOT NULL DEFAULT 0,
           last_review_ts REAL,
           reps INTEGER NOT NULL DEFAULT 0,
           lapses INTEGER NOT NULL DEFAULT 0,
           state INTEGER NOT NULL DEFAULT 0
         );
         CREATE INDEX IF NOT EXISTS idx_cards_due ON cards(due_ts);
         CREATE TABLE IF NOT EXISTS reviews(
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           word_id INTEGER NOT NULL,
           ts INTEGER NOT NULL,
           grade INTEGER NOT NULL,
           delta_s REAL NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS round_log(
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           ts INTEGER NOT NULL,
           session_id INTEGER NOT NULL,
           round_type TEXT NOT NULL,
           correct INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS events(
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           ts INTEGER NOT NULL,
           kind TEXT NOT NULL,
           xp INTEGER NOT NULL DEFAULT 0,
           duration_s INTEGER NOT NULL DEFAULT 0,
           meta TEXT
         );
         CREATE TABLE IF NOT EXISTS badges(
           id TEXT PRIMARY KEY,
           earned_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS mined(
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           es TEXT NOT NULL,
           en TEXT,
           source TEXT NOT NULL DEFAULT '',
           created_at INTEGER NOT NULL,
           in_deck INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS mined_words(
           mined_id INTEGER NOT NULL REFERENCES mined(id),
           word_id INTEGER NOT NULL,
           PRIMARY KEY(mined_id, word_id)
         );",
    )
    .expect("migrate user db");
    conn
}

pub fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [key],
        |r| r.get(0),
    )
    .ok()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )
    .ok();
}
