//! User progress database: schema, migrations, settings.

use rusqlite::Connection;

/// Schema changes, applied in order. A database's `PRAGMA user_version` is the
/// number of entries it has already run, so append new steps and never edit or
/// reorder old ones. Step 1 is the original schema; it uses IF NOT EXISTS
/// because databases from before versioning already have those tables.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
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
];

pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// Tables a progress database must have; a restore refuses files without them.
pub const TABLES: &[&str] = &[
    "settings", "cards", "reviews", "round_log", "events", "badges", "mined", "mined_words",
];

pub fn open(path: &std::path::Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .map_err(|e| e.to_string())?;
    migrate(&mut conn)?;
    Ok(conn)
}

pub fn schema_version(conn: &Connection) -> Result<i64, String> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| e.to_string())
}

/// Brings the schema up to date in one transaction, so a failed step leaves the
/// database as it was.
pub fn migrate(conn: &mut Connection) -> Result<(), String> {
    let version = schema_version(conn)?;
    if version > SCHEMA_VERSION {
        return Err(format!(
            "Este progreso lo guardó una versión más nueva de Tilde (esquema {version}; esta versión conoce hasta el {SCHEMA_VERSION}). Actualiza Tilde para abrirlo."
        ));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for step in &MIGRATIONS[version as usize..] {
        tx.execute_batch(step).map_err(|e| e.to_string())?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tilde_db_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("tilde_user.db")
    }

    #[test]
    fn a_fresh_database_gets_every_table_and_the_current_version() {
        let conn = open(&temp_db("fresh")).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
        for table in TABLES {
            conn.execute(&format!("SELECT * FROM {table}"), []).unwrap();
        }
    }

    #[test]
    fn a_database_from_before_versioning_keeps_its_progress() {
        let path = temp_db("legacy");
        {
            let old = Connection::open(&path).unwrap();
            old.execute_batch(MIGRATIONS[0]).unwrap();
            old.execute("INSERT INTO cards(word_id, state) VALUES (42, 2)", []).unwrap();
            assert_eq!(schema_version(&old).unwrap(), 0);
        }
        let conn = open(&path).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
        let state: i64 = conn.query_row("SELECT state FROM cards WHERE word_id = 42", [], |r| r.get(0)).unwrap();
        assert_eq!(state, 2);
    }

    #[test]
    fn a_database_from_a_newer_app_is_refused_and_left_alone() {
        let path = temp_db("newer");
        {
            let newer = open(&path).unwrap();
            newer.pragma_update(None, "user_version", SCHEMA_VERSION + 1).unwrap();
        }
        let err = open(&path).expect_err("newer schema must be refused");
        assert!(err.contains("más nueva"), "{err}");
        let conn = Connection::open(&path).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION + 1);
    }
}
