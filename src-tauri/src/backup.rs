//! Backup / restore: a zip containing a consistent snapshot of the user DB.

use std::io::Write;
use std::path::{Path, PathBuf};

pub fn export(user: &rusqlite::Connection, app_dir: &Path) -> Result<PathBuf, String> {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let downloads = std::env::var("HOME")
        .map(|h| PathBuf::from(h).join("Downloads"))
        .unwrap_or_else(|_| app_dir.to_path_buf());
    let _ = std::fs::create_dir_all(&downloads);
    let out = downloads.join(format!("tilde-backup-{stamp}.zip"));
    let snapshot = app_dir.join("backup_snapshot.db");
    let _ = std::fs::remove_file(&snapshot);
    user.execute(
        "VACUUM INTO ?1",
        rusqlite::params![snapshot.to_string_lossy().as_ref()],
    )
    .map_err(|e| e.to_string())?;

    let db_bytes = std::fs::read(&snapshot).map_err(|e| e.to_string())?;
    let meta = serde_json::json!({
        "app": "tilde",
        "format": 1,
        "exported_at": chrono::Utc::now().to_rfc3339(),
    });

    let file = std::fs::File::create(&out).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
    zip.start_file("meta.json", opts).map_err(|e| e.to_string())?;
    zip.write_all(meta.to_string().as_bytes()).map_err(|e| e.to_string())?;
    zip.start_file("user.db", opts).map_err(|e| e.to_string())?;
    zip.write_all(&db_bytes).map_err(|e| e.to_string())?;
    zip.finish().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&snapshot);
    Ok(out)
}

/// Restore user progress from a backup zip. The caller must reload the
/// connection afterwards (the DB file is replaced).
pub fn import(app_dir: &Path, bytes: &[u8]) -> Result<(), String> {
    let reader = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;
    let mut db_bytes: Option<Vec<u8>> = None;
    let mut meta_ok = false;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        match f.name().to_string().as_str() {
            "user.db" => {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut f, &mut buf).map_err(|e| e.to_string())?;
                db_bytes = Some(buf);
            }
            "meta.json" => {
                let mut s = String::new();
                std::io::Read::read_to_string(&mut f, &mut s).map_err(|e| e.to_string())?;
                meta_ok = s.contains("\"tilde\"");
            }
            _ => {}
        }
    }
    if !meta_ok {
        return Err("not a Tilde backup".into());
    }
    let db_bytes = db_bytes.ok_or("backup missing user.db")?;

    let target = app_dir.join("tilde_user.db");
    for ext in ["", "-wal", "-shm"] {
        let p = PathBuf::from(format!("{}{ext}", target.display()));
        let _ = std::fs::remove_file(&p);
    }
    std::fs::write(&target, &db_bytes).map_err(|e| e.to_string())?;
    Ok(())
}
