//! Backups: zip files holding a consistent snapshot of the user DB.
//!
//! The file name says what made it, and decides whether it is ever deleted:
//!   tilde-backup-*          "Exportar" in Settings; kept forever
//!   tilde-auto-*            after each session; only the newest KEEP_AUTO stay
//!   tilde-before-reset-*    taken before "Reiniciar progreso"; kept forever
//!   tilde-before-restore-*  taken before a restore; kept forever

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
            let probe = path.join(".tilde-write-test");
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
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stamp = chrono::Local::now().format(STAMP);
    let out = dir.join(format!("{}{stamp}.zip", kind.prefix()));

    // VACUUM INTO next to the live DB (not in the backup folder, which may be
    // watched by a sync tool) gives a consistent copy without blocking writers.
    let scratch = user
        .path()
        .filter(|p| !p.is_empty())
        .and_then(|p| Path::new(p).parent().map(Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir)
        .join(format!(
            ".tilde-snapshot-{}-{}.db",
            std::process::id(),
            SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
    let _ = std::fs::remove_file(&scratch);
    let db_bytes = user
        .execute("VACUUM INTO ?1", [scratch.to_string_lossy().as_ref()])
        .map_err(|e| e.to_string())
        .and_then(|_| std::fs::read(&scratch).map_err(|e| e.to_string()));
    let _ = std::fs::remove_file(&scratch);
    let db_bytes = db_bytes?;

    let meta = serde_json::json!({
        "app": "tilde",
        "format": 1,
        "kind": kind.id(),
        "schema": crate::db::schema_version(user).unwrap_or(0),
        "exported_at": chrono::Utc::now().to_rfc3339(),
    });

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
        return Err(e);
    }

    if kind == Kind::Auto {
        prune(dir, KEEP_AUTO);
    }
    Ok(out)
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

fn remove_db_files(path: &Path, suffixes: &[&str]) {
    for suffix in suffixes {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(p));
    }
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
    // Back to a rollback journal so the file stands alone when moved into place.
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

/// Replaces the live progress DB with the one in `bytes`.
///
/// Nothing about the current progress changes until the backup has been fully
/// checked and a safety copy of the current state written to `safety_dir`; on
/// any error before the swap the live DB is exactly as it was. Returns the
/// safety copy's path.
pub fn restore(
    live: &mut Connection,
    live_path: &Path,
    bytes: &[u8],
    safety_dir: &Path,
) -> Result<PathBuf, String> {
    let mut staged = live_path.as_os_str().to_owned();
    staged.push(".restore");
    let staged = PathBuf::from(staged);
    let all = ["", "-wal", "-shm", "-journal"];
    remove_db_files(&staged, &all);

    let result = (|| {
        unpack(bytes, &staged)?;
        let safety = write(live, safety_dir, Kind::BeforeRestore).map_err(|e| {
            format!("No se pudo guardar una copia del progreso actual antes de restaurar: {e}")
        })?;

        // Closing the live connection checkpoints its WAL; the leftover -wal
        // and -shm must go too, or SQLite would replay them onto the new file.
        let placeholder = Connection::open_in_memory().map_err(|e| e.to_string())?;
        drop(std::mem::replace(live, placeholder));
        remove_db_files(live_path, &["-wal", "-shm"]);
        if let Err(e) = std::fs::rename(&staged, live_path) {
            *live = crate::db::open(live_path)?;
            return Err(e.to_string());
        }
        *live = crate::db::open(live_path)?;
        Ok(safety)
    })();
    remove_db_files(&staged, &all);
    result
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
}
