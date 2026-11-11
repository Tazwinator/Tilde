//! Backups: zip files holding a consistent snapshot of the user DB.
//!
//! The file name says what made it, and decides whether it is ever deleted:
//!   tilde-backup-*          "Exportar" in Settings; kept forever
//!   tilde-auto-*            after each session; only the newest KEEP_AUTO stay
//!   tilde-before-reset-*    taken before "Reiniciar progreso"; kept forever
//!   tilde-before-restore-*  taken before a restore; kept forever

use rusqlite::backup::{Backup, StepResult};
use rusqlite::Connection;
use std::io::Write;
use std::path::{Path, PathBuf};
use tilde_core::types::{BackupFolder, BackupInfo};

pub const KEEP_AUTO: usize = 10;

const STAMP: &str = "%Y%m%d-%H%M%S";

static SCRATCH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Manual,
    Auto,
    BeforeReset,
    BeforeRestore,
}

impl Kind {
    const ALL: [Kind; 4] = [Kind::Manual, Kind::Auto, Kind::BeforeReset, Kind::BeforeRestore];

    fn id(self) -> &'static str {
        match self {
            Kind::Manual => "manual",
            Kind::Auto => "auto",
            Kind::BeforeReset => "before-reset",
            Kind::BeforeRestore => "before-restore",
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            Kind::Manual => "tilde-backup-",
            Kind::Auto => "tilde-auto-",
            Kind::BeforeReset => "tilde-before-reset-",
            Kind::BeforeRestore => "tilde-before-restore-",
        }
    }

    /// The kind and timestamp encoded in a backup's file name.
    fn parse(name: &str) -> Option<(Kind, &str)> {
        let stem = name.strip_suffix(".zip")?;
        Kind::ALL
            .into_iter()
            .find_map(|k| stem.strip_prefix(k.prefix()).map(|stamp| (k, stamp)))
    }
}

// --- where backups go ------------------------------------------------------

// The folder is a property of this computer, not of the progress, so it lives
// in a small file next to the DB rather than in the DB's settings table: a
// restore or reset must not point backups at another machine's path.
const DEVICE_FILE: &str = "device.json";

fn device(app_dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(app_dir.join(DEVICE_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_device(app_dir: &Path, map: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    let path = app_dir.join(DEVICE_FILE);
    let tmp = app_dir.join(format!("{DEVICE_FILE}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(map).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

pub fn default_folder(app_dir: &Path) -> PathBuf {
    app_dir.join("backups")
}

pub fn folder(app_dir: &Path) -> PathBuf {
    device(app_dir)
        .get("backupDir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_folder(app_dir))
}

/// Points backups at `dir`, or back at the default folder for `None`. The
/// folder must be an absolute path we can create and write to.
pub fn set_folder(app_dir: &Path, dir: Option<&str>) -> Result<PathBuf, String> {
    let mut map = device(app_dir);
    match dir.map(str::trim).filter(|d| !d.is_empty()) {
        None => {
            map.remove("backupDir");
        }
        Some(d) => {
            let path = PathBuf::from(d);
            if !path.is_absolute() {
                return Err("Indica la ruta completa de la carpeta.".into());
            }
            std::fs::create_dir_all(&path)
                .map_err(|e| format!("No se pudo crear la carpeta: {e}"))?;
            let probe = path.join(WRITE_TEST);
            std::fs::write(&probe, b"ok")
                .map_err(|e| format!("No se puede escribir en esa carpeta: {e}"))?;
            let _ = std::fs::remove_file(&probe);
            map.insert("backupDir".into(), d.into());
        }
    }
    save_device(app_dir, &map)?;
    Ok(folder(app_dir))
}

pub fn describe(app_dir: &Path) -> BackupFolder {
    let dir = folder(app_dir);
    // so "Mostrar" has something to open before the first backup exists
    let _ = std::fs::create_dir_all(&dir);
    BackupFolder {
        is_default: dir == default_folder(app_dir),
        backups: list(&dir),
        dir: dir.to_string_lossy().into_owned(),
        last_error: None,
    }
}

/// Every Tilde backup in `dir`, newest first.
pub fn list(dir: &Path) -> Vec<BackupInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<BackupInfo> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let (kind, stamp) = Kind::parse(&name)?;
            let meta = e.metadata().ok()?;
            let created_at = chrono::NaiveDateTime::parse_from_str(stamp, STAMP)
                .ok()
                .and_then(|t| t.and_local_timezone(chrono::Local).earliest())
                .map(|t| t.timestamp())
                .or_else(|| {
                    let modified = meta.modified().ok()?;
                    let secs = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
                    i64::try_from(secs).ok()
                })
                .unwrap_or(0);
            Some(BackupInfo {
                path: e.path().to_string_lossy().into_owned(),
                name,
                kind: kind.id().into(),
                created_at,
                size_bytes: i64::try_from(meta.len()).unwrap_or(i64::MAX),
            })
        })
        .collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.name.cmp(&a.name)));
    out
}

// --- writing ---------------------------------------------------------------

/// Snapshots `user` into a new zip in `dir`. Automatic snapshots then prune the
/// folder down to the newest KEEP_AUTO; nothing else is ever deleted.
pub fn write(user: &Connection, dir: &Path, kind: Kind) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear la carpeta de copias: {e}"))?;
    let stamp = chrono::Local::now().format(STAMP);
    let out = dir.join(format!("{}{stamp}.zip", kind.prefix()));

    // The copy is made next to the live DB (not in the backup folder, which
    // may be watched by a sync tool).
    let scratch = user
        .path()
        .filter(|p| !p.is_empty())
        .and_then(|p| Path::new(p).parent().map(Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir)
        .join(format!(
            "{SNAPSHOT_PREFIX}{}-{}.db",
            std::process::id(),
            SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
    remove_db_files(&scratch, &DB_FILES);
    let copied = snapshot(user, &scratch, kind)
        .map_err(|e| copy_error(&e))
        .and_then(|damaged| {
            let bytes = std::fs::read(&scratch).map_err(|e| format!("No se pudo leer la copia: {e}"))?;
            Ok((bytes, damaged))
        });
    remove_db_files(&scratch, &DB_FILES);
    let (db_bytes, damaged) = copied?;

    let mut meta = serde_json::json!({
        "app": "tilde",
        "format": 1,
        "kind": kind.id(),
        "schema": crate::db::schema_version(user).unwrap_or(0),
        "exported_at": chrono::Utc::now().to_rfc3339(),
    });
    if damaged {
        meta["damaged"] = true.into();
    }

    // Written under a temporary name and renamed, so a sync tool never picks
    // up a half-written zip.
    let part = dir.join(format!(".{}{stamp}.zip.part", kind.prefix()));
    let zipped = (|| -> Result<(), String> {
        let file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("meta.json", opts).map_err(|e| e.to_string())?;
        zip.write_all(meta.to_string().as_bytes()).map_err(|e| e.to_string())?;
        zip.start_file("user.db", opts).map_err(|e| e.to_string())?;
        zip.write_all(&db_bytes).map_err(|e| e.to_string())?;
        let file = zip.finish().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&part, &out).map_err(|e| e.to_string())
    })();
    if let Err(e) = zipped {
        let _ = std::fs::remove_file(&part);
        return Err(format!("No se pudo escribir la copia: {e}"));
    }

    if kind == Kind::Auto {
        prune(dir, KEEP_AUTO);
    }
    Ok(out)
}

/// What to tell the player when their progress couldn't be copied for a
/// backup. Not `db::user_error`, which says the progress wasn't saved: here
/// it was, and only the copy of it failed.
fn copy_error(e: &rusqlite::Error) -> String {
    use crate::db::Failure;
    match crate::db::failure(e) {
        Failure::Damaged => "Tu progreso está dañado y no se puede copiar. Cierra y vuelve a abrir Tilde: lo apartará sin borrar nada y recuperará tu última copia.".into(),
        Failure::Busy => crate::db::user_error(e),
        Failure::DiskFull => "No queda espacio en el disco para hacer la copia.".into(),
        Failure::Other => format!("No se pudo copiar tu progreso: {e}"),
    }
}

/// Copies `user` into the file `scratch`. True if it is a raw copy of a
/// damaged database rather than a clean one.
fn snapshot(user: &Connection, scratch: &Path, kind: Kind) -> rusqlite::Result<bool> {
    // VACUUM INTO gives a consistent copy without blocking writers. It reads
    // every row to do it, so it also refuses a damaged database...
    let damage = match user.execute("VACUUM INTO ?1", [scratch.to_string_lossy().as_ref()]) {
        Ok(_) => return Ok(false),
        Err(e) if kind == Kind::BeforeRestore && crate::db::is_damage(&e) => e,
        Err(e) => return Err(e),
    };
    // ...which is right for every backup but the one taken before a restore:
    // that restore is how the player gets out of a damaged database, and
    // what's being replaced must still be kept. So copy it page by page, bad
    // pages and all. Never for the other kinds: a damaged "auto" would push a
    // good one out of the newest KEEP_AUTO.
    eprintln!("the progress being replaced is damaged ({damage}); keeping a raw copy of it");
    remove_db_files(scratch, &DB_FILES);
    let mut raw = Connection::open(scratch)?;
    all_pages(&Backup::new(user, &mut raw)?)?;
    Ok(true)
}

/// Runs a backup-API copy to the end in one step. One step is one write
/// transaction on the destination, so the copy lands whole or not at all.
fn all_pages(copy: &Backup) -> rusqlite::Result<()> {
    match copy.step(-1)? {
        StepResult::Done => Ok(()),
        // the destination's busy timeout has already run out by now
        _ => Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            None,
        )),
    }
}

/// Deletes all but the newest `keep` automatic snapshots in `dir`.
pub fn prune(dir: &Path, keep: usize) {
    let mut autos: Vec<BackupInfo> =
        list(dir).into_iter().filter(|b| b.kind == Kind::Auto.id()).collect();
    for old in autos.drain(..).skip(keep) {
        let _ = std::fs::remove_file(&old.path);
    }
}

// --- restoring -------------------------------------------------------------

/// A database file and what SQLite may leave beside it.
const DB_FILES: [&str; 4] = ["", "-wal", "-shm", "-journal"];

fn remove_db_files(path: &Path, suffixes: &[&str]) {
    for suffix in suffixes {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(p));
    }
}

/// Where a backup is unpacked and checked before it replaces `live_path`.
fn staged_path(live_path: &Path) -> PathBuf {
    let mut staged = live_path.as_os_str().to_owned();
    staged.push(STAGED_SUFFIX);
    PathBuf::from(staged)
}

/// Extracts a backup's user.db to `dest` and checks it is a Tilde progress DB
/// this version can open, migrating it to the current schema.
pub fn unpack(bytes: &[u8], dest: &Path) -> Result<(), String> {
    const NOT_A_BACKUP: &str = "Este archivo no es una copia de seguridad de Tilde.";
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| NOT_A_BACKUP)?;

    let meta: serde_json::Value = {
        let f = zip.by_name("meta.json").map_err(|_| NOT_A_BACKUP)?;
        serde_json::from_reader(f).map_err(|_| NOT_A_BACKUP)?
    };
    if meta.get("app").and_then(|v| v.as_str()) != Some("tilde") {
        return Err(NOT_A_BACKUP.into());
    }

    {
        let mut f = zip
            .by_name("user.db")
            .map_err(|_| "La copia no contiene ningún progreso (falta user.db).")?;
        let mut out = std::fs::File::create(dest).map_err(|e| e.to_string())?;
        std::io::copy(&mut f, &mut out).map_err(|e| format!("La copia está dañada: {e}"))?;
    }

    let damaged = |e: rusqlite::Error| format!("La copia está dañada: {e}");
    let mut conn = Connection::open(dest).map_err(damaged)?;
    // Back to a rollback journal, so the file stands alone.
    conn.query_row("PRAGMA journal_mode=DELETE", [], |_| Ok(()))
        .map_err(damaged)?;
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(damaged)?;
    if integrity != "ok" {
        return Err(format!("La copia está dañada: {integrity}"));
    }
    for table in crate::db::TABLES {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [table],
                |r| r.get(0),
            )
            .map_err(damaged)?;
        if !exists {
            return Err(format!("La copia está incompleta (falta la tabla {table})."));
        }
    }
    crate::db::migrate(&mut conn)
}

/// Unpacks and checks a backup into `staged`, ready for `copy_into(live, ..)`.
fn stage(bytes: &[u8], staged: &Path, live: &Connection) -> Result<(), String> {
    unpack(bytes, staged)?;
    // The copy can't change the page size of a database in WAL mode, as the
    // live one is, so the staged file takes the live one's size first. (A
    // backup made on another build of SQLite may have another.)
    let fit = (|| -> rusqlite::Result<()> {
        let page_size = |c: &Connection| c.query_row("PRAGMA page_size", [], |r| r.get::<_, i64>(0));
        let conn = Connection::open(staged)?;
        let wanted = page_size(live)?;
        if page_size(&conn)? != wanted {
            conn.execute_batch(&format!("PRAGMA page_size = {wanted}; VACUUM;"))?;
        }
        Ok(())
    })();
    fit.map_err(|e| format!("No se pudo preparar la copia: {e}"))
}

/// Replaces everything in `live` with the staged database, through SQLite's
/// backup API rather than by swapping files: the connection stays the same
/// one, and so does the file, so any other process that has it open (a second
/// Tilde that slipped past the instance lock) sees the restored progress too
/// instead of writing into a file that is no longer there. All or nothing.
fn copy_into(live: &mut Connection, staged: &Path) -> Result<(), String> {
    let copied = (|| {
        let from = Connection::open_with_flags(staged, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let copy = Backup::new(&from, live)?;
        all_pages(&copy)
    })();
    copied.map_err(|e| crate::db::user_error(&e))?;
    // Only so the WAL doesn't stay as big as the whole database.
    let _ = live.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    Ok(())
}

/// Replaces the live progress DB with the one in `bytes`.
///
/// Nothing about the current progress changes until the backup has been fully
/// checked and a safety copy of the current state written to `safety_dir`; on
/// any error the live DB is exactly as it was, and no safety copy stays
/// behind. Returns the safety copy's path.
pub fn restore(
    live: &mut Connection,
    live_path: &Path,
    bytes: &[u8],
    safety_dir: &Path,
) -> Result<PathBuf, String> {
    let staged = staged_path(live_path);
    remove_db_files(&staged, &DB_FILES);

    let result = (|| {
        stage(bytes, &staged, live)?;
        let safety = write(live, safety_dir, Kind::BeforeRestore).map_err(|e| {
            format!("No se pudo guardar una copia del progreso actual antes de restaurar: {e}")
        })?;
        if let Err(e) = copy_into(live, &staged) {
            // Nothing was replaced: the copy holds what is still live, and
            // isn't "from before a restore". Kept, it would be the newest
            // backup, the one `recover_newest` stops at.
            let _ = std::fs::remove_file(&safety);
            return Err(e);
        }
        Ok(safety)
    })();
    remove_db_files(&staged, &DB_FILES);
    result
}

/// For startup, after damaged progress was set aside and `fresh` started in
/// its place: fills it from the newest backup in `dir` that restores, and
/// says which one that was.
pub fn recover_newest(fresh: &mut Connection, dir: &Path) -> Option<BackupInfo> {
    let staged = staged_path(Path::new(fresh.path().filter(|p| !p.is_empty())?));
    for backup in list(dir) {
        // The last thing the player did was reset, or restore some other
        // copy, on purpose. An older automatic backup would undo that, and
        // the choice is theirs to make in Settings.
        if backup.kind == Kind::BeforeReset.id() || backup.kind == Kind::BeforeRestore.id() {
            return None;
        }
        remove_db_files(&staged, &DB_FILES);
        let restored = std::fs::read(&backup.path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| stage(&bytes, &staged, fresh))
            .and_then(|()| copy_into(fresh, &staged));
        remove_db_files(&staged, &DB_FILES);
        match restored {
            Ok(()) => return Some(backup),
            Err(e) => eprintln!("could not recover {}: {e}", backup.name),
        }
    }
    None
}

// --- cleaning up -----------------------------------------------------------

const SNAPSHOT_PREFIX: &str = ".tilde-snapshot-";
const STAGED_SUFFIX: &str = ".restore";
const WRITE_TEST: &str = ".tilde-write-test";

fn digits(s: &str, n: Option<usize>) -> bool {
    !s.is_empty() && n.is_none_or(|n| s.len() == n) && s.bytes().all(|b| b.is_ascii_digit())
}

/// True for the temporary files this module makes next to the progress DB:
/// a snapshot being taken, a backup staged for a restore, `device.json` being
/// saved. The database itself, and one set aside as damaged, never match.
fn is_temp_in_app_dir(name: &str) -> bool {
    if name == format!("{DEVICE_FILE}.tmp") {
        return true;
    }
    let db_file = |rest: &str| DB_FILES.contains(&rest);
    if let Some(rest) = name.strip_prefix(crate::db::FILE_NAME).and_then(|r| r.strip_prefix(STAGED_SUFFIX)) {
        return db_file(rest);
    }
    name.strip_prefix(SNAPSHOT_PREFIX)
        .and_then(|rest| rest.split_once(".db"))
        .is_some_and(|(ids, rest)| {
            db_file(rest) && ids.split_once('-').is_some_and(|(pid, n)| digits(pid, None) && digits(n, None))
        })
}

/// True for what `write` and `set_folder` leave in the backup folder when
/// killed: a zip not yet renamed into place, the write probe. The folder may
/// be the player's own synced one, so nothing else in it is ours to touch.
fn is_temp_in_backup_dir(name: &str) -> bool {
    if name == WRITE_TEST {
        return true;
    }
    name.strip_prefix('.')
        .and_then(|rest| rest.strip_suffix(".zip.part"))
        .and_then(|rest| Kind::ALL.into_iter().find_map(|k| rest.strip_prefix(k.prefix())))
        .and_then(|stamp| stamp.split_once('-'))
        .is_some_and(|(day, time)| digits(day, Some(8)) && digits(time, Some(6)))
}

/// Deletes the temporary files a killed run left behind: half-made wavs in
/// the audio cache, snapshots and staged restores next to the DB, unfinished
/// zips in the backup folder. Only plain files with exactly those names, and
/// only ones last written before `older_than`, so nothing a running command
/// is still using. Never anything else, and never into a subfolder.
pub fn sweep_stale(app_dir: &Path, cache_dir: &Path, backup_dir: &Path, older_than: std::time::SystemTime) {
    sweep(cache_dir, crate::tts::is_temp_name, older_than);
    sweep(app_dir, is_temp_in_app_dir, older_than);
    sweep(backup_dir, is_temp_in_backup_dir, older_than);
}

fn sweep(dir: &Path, is_temp: fn(&str) -> bool, older_than: std::time::SystemTime) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        if !entry.file_name().to_str().is_some_and(is_temp) {
            continue;
        }
        // not a folder, and not a link to someone else's file
        let stale = std::fs::symlink_metadata(entry.path())
            .is_ok_and(|m| m.file_type().is_file() && m.modified().is_ok_and(|t| t < older_than));
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pruning_keeps_the_newest_automatic_snapshots_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("tilde_prune_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let autos: Vec<String> = (10..23).map(|d| format!("tilde-auto-202610{d}-120000.zip")).collect();
        let keep = [
            "tilde-backup-20261001-090000.zip",
            "tilde-before-reset-20261001-090000.zip",
            "tilde-before-restore-20261001-090000.zip",
            "notes.txt",
        ];
        for name in autos.iter().map(String::as_str).chain(keep) {
            std::fs::write(dir.join(name), b"x").unwrap();
        }

        prune(&dir, KEEP_AUTO);

        let left: Vec<String> = list(&dir).into_iter().filter(|b| b.kind == "auto").map(|b| b.name).collect();
        let newest: Vec<String> = autos.iter().rev().take(KEEP_AUTO).cloned().collect();
        assert_eq!(left, newest);
        for name in keep {
            assert!(dir.join(name).exists(), "{name} was deleted");
        }
    }

    #[test]
    fn stale_temp_files_are_swept_and_nothing_else() {
        let root = std::env::temp_dir().join(format!("tilde_sweep_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (app, cache, backups) = (root.join("app"), root.join("app").join("audio_cache"), root.join("Sync"));
        for dir in [&cache, &backups] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let sha = "0123456789abcdef0123456789abcdef01234567";

        // what a killed run can leave behind
        let temporary = [
            cache.join(format!("{sha}.wav.4242-7.tmp")),
            app.join(".tilde-snapshot-4242-0.db"),
            app.join(".tilde-snapshot-4242-0.db-journal"),
            app.join(".tilde-snapshot-4242-0.db-wal"),
            app.join(".tilde-snapshot-4242-0.db-shm"),
            app.join("device.json.tmp"),
            app.join("tilde_user.db.restore"),
            app.join("tilde_user.db.restore-journal"),
            app.join("tilde_user.db.restore-wal"),
            app.join("tilde_user.db.restore-shm"),
            backups.join(".tilde-backup-20260101-000000.zip.part"),
            backups.join(".tilde-auto-20260101-000000.zip.part"),
            backups.join(".tilde-before-reset-20260101-000000.zip.part"),
            backups.join(".tilde-before-restore-20260101-000000.zip.part"),
            backups.join(".tilde-write-test"),
        ];
        // what must outlive any sweep, however old
        let precious = [
            app.join("tilde_user.db"),
            app.join("tilde_user.db-wal"),
            app.join("tilde_user.db.damaged-20260101-000000"),
            app.join("tilde_user.db.damaged-20260101-000000-wal"),
            app.join("tilde_user.db.restored"),
            app.join("device.json"),
            app.join("tilde.lock"),
            app.join("notes.tmp"),
            app.join(".tilde-snapshot-4242-x.db"),
            app.join(".tilde-snapshot-4242-0.db.bak"),
            // right name, wrong folder
            app.join(".tilde-auto-20260101-000000.zip.part"),
            app.join(format!("{sha}.wav.4242-7.tmp")),
            cache.join(format!("{sha}.wav")),
            cache.join(format!("{sha}.wav.tmp")),
            cache.join("hola.wav.4242-7.tmp"),
            cache.join("notes.tmp"),
            cache.join("tilde_user.db.restore"),
            backups.join("tilde-auto-20260101-000000.zip"),
            backups.join("tilde-backup-20260101-000000.zip"),
            backups.join(".tilde-other.zip.part"),
            backups.join(".tilde-auto-2026-000000.zip.part"),
            backups.join(".tilde-auto-20260101-000000.zip.partial"),
            backups.join("device.json.tmp"),
            backups.join("notes.txt"),
        ];
        // a folder named like a temp file, with something in it
        let folders = [cache.join(format!("{sha}.wav.1-1.tmp")), app.join(".tilde-snapshot-1-1.db"), backups.join(".tilde-auto-20250101-000000.zip.part")];
        for file in temporary.iter().chain(&precious) {
            std::fs::write(file, b"x").unwrap();
        }
        for folder in &folders {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::write(folder.join("inside.tmp"), b"x").unwrap();
        }

        let now = std::time::SystemTime::now();
        let hour_ago = now - std::time::Duration::from_secs(3600);
        // freshly written: a command may still be using them
        sweep_stale(&app, &cache, &backups, hour_ago);
        for file in temporary.iter().chain(&precious) {
            assert!(file.exists(), "{} was deleted while still fresh", file.display());
        }

        let two_hours_ago = now - std::time::Duration::from_secs(7200);
        for path in temporary.iter().chain(&precious) {
            std::fs::File::options().write(true).open(path).unwrap().set_modified(two_hours_ago).unwrap();
        }
        for folder in &folders {
            // where a folder can't be opened like this, it just stays "new"
            let _ = std::fs::File::open(folder).and_then(|f| f.set_modified(two_hours_ago));
        }
        sweep_stale(&app, &cache, &backups, hour_ago);
        for file in &temporary {
            assert!(!file.exists(), "{} was left behind", file.display());
        }
        for file in &precious {
            assert!(file.exists(), "{} was deleted", file.display());
        }
        for folder in &folders {
            assert!(folder.join("inside.tmp").exists(), "{} was deleted", folder.display());
        }
        // a folder that isn't there is not an error
        sweep_stale(&root.join("gone"), &root.join("gone"), &root.join("gone"), now);
    }

    fn progress_with_cards(path: &Path, cards: i64) -> Connection {
        let conn = crate::db::open(path).unwrap();
        conn.execute(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
             INSERT INTO cards(word_id, state) SELECT i, 2 FROM n WHERE ?1 > 0",
            [cards],
        )
        .unwrap();
        conn
    }

    fn cards(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM cards", [], |r| r.get(0)).unwrap()
    }

    fn meta_of(zip_path: &Path) -> serde_json::Value {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(zip_path).unwrap()).unwrap();
        serde_json::from_reader(zip.by_name("meta.json").unwrap()).unwrap()
    }

    #[test]
    fn only_the_copy_before_a_restore_may_hold_a_damaged_database() {
        let dir = std::env::temp_dir().join(format!("tilde_raw_copy_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("tilde_user.db");
        drop(progress_with_cards(&path, 20));
        crate::db::damage_page(&path, "cards");
        let live = crate::db::connect(&path).unwrap();
        let backups = dir.join("backups");

        for kind in [Kind::Auto, Kind::Manual, Kind::BeforeReset] {
            let err = write(&live, &backups, kind).expect_err("a damaged database was backed up as if it were good");
            assert!(err.starts_with("Tu progreso está dañado y no se puede copiar."), "{err}");
        }
        assert!(list(&backups).is_empty());

        let raw = write(&live, &backups, Kind::BeforeRestore).unwrap();
        assert_eq!(meta_of(&raw)["damaged"], true);
        // every page of it, the bad one included
        let staged = dir.join("staged.db");
        let err = unpack(&std::fs::read(&raw).unwrap(), &staged).expect_err("the raw copy passed for a good backup");
        assert!(err.starts_with("La copia está dañada"), "{err}");
        // nothing temporary is left next to the database
        let left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(SNAPSHOT_PREFIX))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn a_backup_that_fails_is_not_reported_as_progress_that_was_not_saved() {
        use rusqlite::ffi::{SQLITE_BUSY, SQLITE_CORRUPT, SQLITE_FULL, SQLITE_IOERR};
        let message = |code| copy_error(&rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None));
        assert_eq!(message(SQLITE_FULL), "No queda espacio en el disco para hacer la copia.");
        assert!(message(SQLITE_CORRUPT).starts_with("Tu progreso está dañado y no se puede copiar."));
        assert!(message(SQLITE_BUSY).starts_with("Otro programa"));
        assert!(message(SQLITE_IOERR).starts_with("No se pudo copiar tu progreso: "));
        // the progress itself was saved all along; only `db::user_error` says otherwise
        for code in [SQLITE_FULL, SQLITE_CORRUPT, SQLITE_BUSY, SQLITE_IOERR] {
            assert!(!message(code).contains("guardar"), "{}", message(code));
        }
    }

    #[test]
    fn a_good_backup_is_not_marked_damaged() {
        let dir = std::env::temp_dir().join(format!("tilde_good_copy_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let live = progress_with_cards(&dir.join("tilde_user.db"), 3);
        let zip = write(&live, &dir.join("backups"), Kind::BeforeRestore).unwrap();
        assert!(meta_of(&zip).get("damaged").is_none());
    }

    #[test]
    fn a_restore_that_fails_leaves_the_progress_as_it_was() {
        let dir = std::env::temp_dir().join(format!("tilde_restore_busy_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("tilde_user.db");
        let backups = dir.join("backups");
        let mut live = progress_with_cards(&path, 5);
        let other = progress_with_cards(&dir.join("other.db"), 9);
        let zip = std::fs::read(write(&other, &backups, Kind::Manual).unwrap()).unwrap();

        // another program is in the middle of a write to the live database
        let writer = crate::db::connect(&path).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE; INSERT INTO settings(key, value) VALUES ('k', 'v');").unwrap();
        live.busy_timeout(std::time::Duration::from_millis(50)).unwrap();
        let err = restore(&mut live, &path, &zip, &backups).expect_err("restored over a database in use");
        assert!(err.contains("Otro programa"), "{err}");
        assert_eq!(cards(&live), 5);
        assert!(!staged_path(&path).exists());
        // Nothing was replaced, so no copy "from before a restore" is left:
        // it would be the newest backup, and the one startup recovery stops at.
        let kinds = |dir: &Path| -> Vec<String> { list(dir).into_iter().map(|b| b.kind).collect() };
        assert_eq!(kinds(&backups), ["manual"]);
        let mut fresh = crate::db::open(&dir.join("fresh").join("tilde_user.db")).unwrap();
        assert!(recover_newest(&mut fresh, &backups).is_some(), "the failed restore got in the way of a recovery");

        writer.execute_batch("COMMIT").unwrap();
        restore(&mut live, &path, &zip, &backups).unwrap();
        assert_eq!(cards(&live), 9);
        assert_eq!(kinds(&backups), ["before-restore", "manual"]);
        // the other program sees the restored progress, in the same file
        assert_eq!(cards(&writer), 9);
        let mode: String = live.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn a_restore_replaces_a_much_larger_database_cleanly() {
        let dir = std::env::temp_dir().join(format!("tilde_restore_shrink_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("tilde_user.db");
        let backups = dir.join("backups");
        let small = progress_with_cards(&dir.join("small.db"), 2);
        small.execute("INSERT INTO settings(key, value) VALUES ('definition_lang', 'es')", []).unwrap();
        let zip = std::fs::read(write(&small, &backups, Kind::Manual).unwrap()).unwrap();

        let mut live = progress_with_cards(&path, 20_000);
        live.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
        let size_before = std::fs::metadata(&path).unwrap().len();
        assert!(size_before > 500_000);
        restore(&mut live, &path, &zip, &backups).unwrap();
        assert_eq!(cards(&live), 2);
        assert_eq!(crate::db::get_setting(&live, "definition_lang").as_deref(), Some("es"));
        assert!(matches!(crate::db::check(&live), crate::db::Health::Ok));
        // still the connection it was: foreign keys on, writes land
        let foreign_keys: i64 = live.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(foreign_keys, 1);
        live.execute("INSERT INTO cards(word_id) VALUES (99)", []).unwrap();
        drop(live);

        // and on disk, for whoever opens the file next
        assert!(std::fs::metadata(&path).unwrap().len() < size_before / 4);
        let reopened = crate::db::open_or_set_aside(&path).unwrap();
        assert!(reopened.1.is_none(), "the restored database doesn't pass the startup check");
        assert_eq!(cards(&reopened.0), 3);
        // the copy of what was replaced holds all of it
        let before = list(&backups).into_iter().find(|b| b.kind == "before-restore").unwrap();
        let staged = dir.join("check.db");
        unpack(&std::fs::read(&before.path).unwrap(), &staged).unwrap();
        assert_eq!(cards(&Connection::open(&staged).unwrap()), 20_000);
    }
}
