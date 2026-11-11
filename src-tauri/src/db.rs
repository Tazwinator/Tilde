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

/// The progress database's file name inside the app data directory.
pub const FILE_NAME: &str = "tilde_user.db";

/// Opens the file and sets the connection up, without touching the schema.
pub fn connect(path: &std::path::Path) -> rusqlite::Result<Connection> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    Ok(conn)
}

pub fn open(path: &std::path::Path) -> Result<Connection, String> {
    let mut conn = connect(path).map_err(|e| e.to_string())?;
    migrate(&mut conn)?;
    Ok(conn)
}

pub enum Health {
    Ok,
    Damaged,
    /// The check itself couldn't run (the file is busy, the disk won't read).
    /// That proves nothing about the database.
    Unknown(String),
}

/// True for the errors SQLite gives when the file itself is broken.
pub fn is_damage(e: &rusqlite::Error) -> bool {
    use rusqlite::ErrorCode::{DatabaseCorrupt, NotADatabase};
    matches!(e.sqlite_error_code(), Some(DatabaseCorrupt | NotADatabase))
}

/// Reads every page of every table and index once (`quick_check`): a bad page
/// anywhere is found here rather than by the first write that lands on it.
pub fn check(conn: &Connection) -> Health {
    match conn.query_row("PRAGMA quick_check(1)", [], |r| r.get::<_, String>(0)) {
        Ok(result) if result == "ok" => Health::Ok,
        Ok(_) => Health::Damaged,
        Err(e) if is_damage(&e) => Health::Damaged,
        Err(e) => Health::Unknown(e.to_string()),
    }
}

/// Opens the progress DB at startup, when there's no UI yet to restore from.
///
/// The file is checked before anything is written to it. One that's damaged
/// (not a database, or with a bad page anywhere) is moved aside, never
/// deleted, and a fresh database started in its place, so Tilde still opens;
/// the second value says where it went, and the caller then recovers the
/// newest backup into the fresh one. A file is only set aside on proof: a
/// check that couldn't run, or any other error, a file from a newer Tilde
/// included, leaves it alone.
pub fn open_or_set_aside(path: &std::path::Path) -> Result<(Connection, Option<std::path::PathBuf>), String> {
    let conn = match connect(path) {
        Ok(conn) => conn,
        Err(e) if is_damage(&e) => return set_aside(path),
        Err(e) => return Err(e.to_string()),
    };
    let health = check(&conn);
    open_checked(path, conn, health)
}

/// The rest of `open_or_set_aside`, once the check has given its verdict on
/// the freshly opened `conn`.
fn open_checked(
    path: &std::path::Path,
    mut conn: Connection,
    health: Health,
) -> Result<(Connection, Option<std::path::PathBuf>), String> {
    match health {
        Health::Ok => {}
        Health::Damaged => {
            drop(conn);
            return set_aside(path);
        }
        Health::Unknown(e) => eprintln!("could not check the progress database: {e}"),
    }
    match migrate(&mut conn) {
        Ok(()) => Ok((conn, None)),
        Err(e) => {
            drop(conn);
            if damaged(path) {
                set_aside(path)
            } else {
                Err(e)
            }
        }
    }
}

/// Renames the database (and its WAL files) out of the way and starts a new
/// one. No connection to it may be open.
fn set_aside(path: &std::path::Path) -> Result<(Connection, Option<std::path::PathBuf>), String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let with = |p: &std::path::Path, suffix: &str| {
        let mut s = p.as_os_str().to_owned();
        s.push(suffix);
        std::path::PathBuf::from(s)
    };
    let aside = with(path, &format!(".damaged-{stamp}"));
    std::fs::rename(path, &aside).map_err(|e| e.to_string())?;
    for ext in ["-wal", "-shm"] {
        let _ = std::fs::rename(with(path, ext), with(&aside, ext));
    }
    Ok((open(path)?, Some(aside)))
}

fn damaged(path: &std::path::Path) -> bool {
    Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map(|c| matches!(check(&c), Health::Damaged))
        .unwrap_or_else(|e| is_damage(&e))
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

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

/// Runs `work` as one write transaction: every write in it is saved or, on
/// any error, none is. A command puts all of its writes in one of these, so a
/// failure can't leave half of an answer, an import or a placement behind.
pub fn in_transaction<T>(
    conn: &mut Connection,
    work: impl FnOnce(&Connection) -> rusqlite::Result<T>,
) -> rusqlite::Result<T> {
    // IMMEDIATE takes the write lock up front, where the busy timeout applies.
    // A deferred transaction that reads first can't wait when it comes to
    // write: SQLite fails it at once if another connection wrote in between.
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let out = {
        let _until_done = NoCommits::on(&tx);
        work(&tx)
    }?;
    tx.commit()?;
    Ok(out)
}

/// While this lives, nothing can be committed on the connection.
///
/// A full disk, an I/O error, even one in a read: on those SQLite ends the
/// whole transaction by itself, not just the statement. If that error goes
/// unnoticed, whatever is written next has no transaction to belong to and
/// would be saved on its own, with the rest of the command undone. Under
/// this guard such a write fails instead, and nothing of the command stays.
struct NoCommits<'a>(&'a Connection);

impl<'a> NoCommits<'a> {
    fn on(conn: &'a Connection) -> Self {
        // answering true turns the commit into a rollback
        conn.commit_hook(Some(|| true));
        NoCommits(conn)
    }
}

impl Drop for NoCommits<'_> {
    // On a panic too: left behind, it would refuse every save from then on.
    fn drop(&mut self) {
        self.0.commit_hook(None::<fn() -> bool>);
    }
}

/// The failures the player can be told about in plain words; for the rest
/// they get SQLite's.
pub enum Failure {
    Damaged,
    Busy,
    DiskFull,
    Other,
}

pub fn failure(e: &rusqlite::Error) -> Failure {
    use rusqlite::ErrorCode::{DatabaseBusy, DatabaseLocked, DiskFull};
    if is_damage(e) {
        return Failure::Damaged;
    }
    match e.sqlite_error_code() {
        Some(DatabaseBusy | DatabaseLocked) => Failure::Busy,
        Some(DiskFull) => Failure::DiskFull,
        _ => Failure::Other,
    }
}

/// What to tell the player when a write to their progress fails.
pub fn user_error(e: &rusqlite::Error) -> String {
    match failure(e) {
        Failure::Damaged => "Tu progreso está dañado y no se pudo guardar. Cierra y vuelve a abrir Tilde: lo apartará sin borrar nada y recuperará tu última copia.".into(),
        Failure::Busy => "Otro programa está usando tu progreso; inténtalo de nuevo en un momento.".into(),
        Failure::DiskFull => "No queda espacio en el disco; no se pudo guardar tu progreso.".into(),
        Failure::Other => format!("No se pudo guardar tu progreso: {e}"),
    }
}

/// Overwrites the root page of a table or index with junk, as a failing disk
/// might, leaving the rest of the file readable. No connection may be open.
#[cfg(test)]
pub(crate) fn damage_page(path: &std::path::Path, object: &str) {
    use std::io::{Seek, SeekFrom, Write};
    let (root, page_size): (u64, u64) = {
        let conn = Connection::open(path).unwrap();
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
        let root = conn
            .query_row("SELECT rootpage FROM sqlite_master WHERE name = ?1", [object], |r| r.get(0))
            .unwrap();
        (root, conn.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap())
    };
    let mut file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.seek(SeekFrom::Start((root - 1) * page_size)).unwrap();
    file.write_all(&vec![0xFF; page_size as usize]).unwrap();
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
    fn a_damaged_database_is_set_aside_and_a_fresh_one_started() {
        let path = temp_db("damaged");
        std::fs::write(&path, b"garbage where progress used to be").unwrap();
        let (conn, aside) = open_or_set_aside(&path).unwrap();
        let aside = aside.expect("the damaged file was set aside");
        assert_eq!(std::fs::read(&aside).unwrap(), b"garbage where progress used to be");
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);

        let healthy = temp_db("healthy");
        drop(open(&healthy).unwrap());
        assert!(open_or_set_aside(&healthy).unwrap().1.is_none());
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
        let err = open_or_set_aside(&path).expect_err("not damaged, just newer");
        assert!(err.contains("más nueva"), "{err}");
        let conn = Connection::open(&path).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION + 1);
    }

    /// A progress DB with some cards in it, closed, with `object`'s root page
    /// then overwritten. The header is fine, so the file still opens.
    fn damaged_db(name: &str, object: &str) -> std::path::PathBuf {
        let path = temp_db(name);
        {
            let conn = open(&path).unwrap();
            for word_id in 1..=50 {
                conn.execute("INSERT INTO cards(word_id, due_ts) VALUES (?1, ?1)", [word_id]).unwrap();
            }
        }
        damage_page(&path, object);
        path
    }

    fn assert_set_aside_untouched(path: &std::path::Path) {
        let damaged_bytes = std::fs::read(path).unwrap();
        let (conn, aside) = open_or_set_aside(path).unwrap();
        let aside = aside.expect("the damaged file was set aside");
        assert_eq!(std::fs::read(&aside).unwrap(), damaged_bytes, "the file set aside was changed");
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
        let cards: i64 = conn.query_row("SELECT COUNT(*) FROM cards", [], |r| r.get(0)).unwrap();
        assert_eq!(cards, 0);
    }

    #[test]
    fn a_partly_damaged_database_is_set_aside_at_startup() {
        let path = damaged_db("bad_table", "cards");
        // what the old startup did, and all it did, still works on this file
        assert!(Connection::open(&path).unwrap().execute_batch("PRAGMA journal_mode=WAL").is_ok());
        assert_set_aside_untouched(&path);
    }

    #[test]
    fn a_damaged_index_page_alone_sets_the_database_aside() {
        assert_set_aside_untouched(&damaged_db("bad_index", "idx_cards_due"));
    }

    #[test]
    fn damaged_progress_from_a_newer_app_is_set_aside_like_any_other() {
        let path = damaged_db("bad_newer", "reviews");
        Connection::open(&path).unwrap().pragma_update(None, "user_version", SCHEMA_VERSION + 1).unwrap();
        // nothing is lost by the rename, and nothing could be read from it here anyway
        assert!(open_or_set_aside(&path).unwrap().1.is_some());
    }

    #[test]
    fn a_check_that_cannot_run_proves_nothing() {
        let path = temp_db("busy");
        let holder = Connection::open(&path).unwrap();
        holder.execute_batch("CREATE TABLE t(x); BEGIN EXCLUSIVE;").unwrap();
        let other = Connection::open(&path).unwrap();
        // fail at once rather than after rusqlite's default 5 s
        other.busy_timeout(std::time::Duration::ZERO).unwrap();
        assert!(matches!(check(&other), Health::Unknown(_)), "a locked file was judged");
        holder.execute_batch("COMMIT").unwrap();
        assert!(matches!(check(&other), Health::Ok));
    }

    #[test]
    fn progress_is_not_set_aside_when_the_check_could_not_run() {
        let path = temp_db("unchecked");
        {
            let conn = open(&path).unwrap();
            conn.execute("INSERT INTO cards(word_id, state) VALUES (42, 2)", []).unwrap();
        }
        // The file was busy, or the disk wouldn't read: `check` says so (the
        // test above), and startup must take that for what it is, no proof.
        let unknown = Health::Unknown("database is locked".into());
        let (conn, aside) = open_checked(&path, connect(&path).unwrap(), unknown).unwrap();
        assert_eq!(aside, None, "progress was set aside without proof of damage");
        let cards: i64 = conn.query_row("SELECT COUNT(*) FROM cards", [], |r| r.get(0)).unwrap();
        assert_eq!(cards, 1);
        let beside: Vec<String> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".damaged-"))
            .collect();
        assert!(beside.is_empty(), "{beside:?}");

        // even when what comes next fails too: still no proof, so still no rename
        drop(conn);
        Connection::open(&path).unwrap().pragma_update(None, "user_version", SCHEMA_VERSION + 1).unwrap();
        let unknown = Health::Unknown("disk I/O error".into());
        let err = open_checked(&path, connect(&path).unwrap(), unknown).expect_err("a newer schema");
        assert!(err.contains("más nueva"), "{err}");
        assert!(path.exists());
    }

    #[test]
    fn set_setting_reports_a_failed_write() {
        let conn = open(&temp_db("setting")).unwrap();
        set_setting(&conn, "session_length", "quick").unwrap();
        conn.execute_batch(
            "CREATE TEMP TRIGGER no_insert BEFORE INSERT ON main.settings BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;
             CREATE TEMP TRIGGER no_update BEFORE UPDATE ON main.settings BEGIN SELECT RAISE(ABORT, 'disk on fire'); END;",
        )
        .unwrap();
        let err = set_setting(&conn, "session_length", "deep").expect_err("the write failed");
        assert!(user_error(&err).starts_with("No se pudo guardar tu progreso"), "{}", user_error(&err));
        assert!(set_setting(&conn, "definition_lang", "es").is_err());
        assert_eq!(get_setting(&conn, "session_length").as_deref(), Some("quick"));
    }

    #[test]
    fn a_failed_transaction_saves_none_of_it() {
        let mut conn = open(&temp_db("transaction")).unwrap();
        let failed = in_transaction(&mut conn, |tx| {
            set_setting(tx, "definition_lang", "es")?;
            tx.execute("INSERT INTO no_such_table VALUES (1)", [])?;
            Ok(())
        });
        assert!(failed.is_err());
        assert_eq!(get_setting(&conn, "definition_lang"), None);
        in_transaction(&mut conn, |tx| set_setting(tx, "definition_lang", "es")).unwrap();
        assert_eq!(get_setting(&conn, "definition_lang").as_deref(), Some("es"));
    }

    #[test]
    fn nothing_is_saved_after_sqlite_ends_the_transaction_itself() {
        let mut conn = open(&temp_db("ended_early")).unwrap();
        // What SQLite does when the disk fills up or a read fails mid-way:
        // the whole transaction goes, not just the statement.
        conn.execute_batch(
            "CREATE TEMP TRIGGER disk_full BEFORE INSERT ON main.badges BEGIN SELECT RAISE(ROLLBACK, 'disk full'); END;",
        )
        .unwrap();
        let failed = in_transaction(&mut conn, |tx| {
            set_setting(tx, "definition_lang", "es")?;
            // an error nobody looks at...
            let _ = tx.execute("INSERT INTO badges(id, earned_at) VALUES ('combo-10', 0)", []);
            // ...and then a write with no transaction left to belong to
            set_setting(tx, "session_length", "deep")
        });
        assert!(failed.is_err());
        assert_eq!(get_setting(&conn, "definition_lang"), None);
        assert_eq!(get_setting(&conn, "session_length"), None, "saved on its own, without the rest");

        // and the connection saves again afterwards, in a transaction or not
        conn.execute_batch("DROP TRIGGER disk_full").unwrap();
        in_transaction(&mut conn, |tx| set_setting(tx, "session_length", "deep")).unwrap();
        set_setting(&conn, "definition_lang", "es").unwrap();
        assert_eq!(get_setting(&conn, "session_length").as_deref(), Some("deep"));
        assert_eq!(get_setting(&conn, "definition_lang").as_deref(), Some("es"));
    }

    #[test]
    fn a_panic_inside_a_transaction_does_not_stop_later_saves() {
        let mut conn = open(&temp_db("panicked")).unwrap();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = in_transaction(&mut conn, |tx| -> rusqlite::Result<()> {
                set_setting(tx, "definition_lang", "es")?;
                panic!("simulated bug in a command");
            });
        }));
        assert!(panicked.is_err());
        assert_eq!(get_setting(&conn, "definition_lang"), None);
        set_setting(&conn, "definition_lang", "en").unwrap();
        in_transaction(&mut conn, |tx| set_setting(tx, "session_length", "deep")).unwrap();
        assert_eq!(get_setting(&conn, "session_length").as_deref(), Some("deep"));
    }

    #[test]
    fn write_errors_are_explained_in_spanish() {
        let failure = |code| rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None);
        assert!(user_error(&failure(rusqlite::ffi::SQLITE_CORRUPT)).contains("está dañado"));
        assert!(user_error(&failure(rusqlite::ffi::SQLITE_NOTADB)).contains("está dañado"));
        assert!(user_error(&failure(rusqlite::ffi::SQLITE_BUSY)).contains("Otro programa"));
        assert!(user_error(&failure(rusqlite::ffi::SQLITE_LOCKED)).contains("Otro programa"));
        assert!(user_error(&failure(rusqlite::ffi::SQLITE_FULL)).contains("No queda espacio"));
        assert!(user_error(&failure(rusqlite::ffi::SQLITE_IOERR)).starts_with("No se pudo guardar tu progreso: "));
    }
}
