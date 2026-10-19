//! Transaction-consistent SQLite snapshots without copying a live WAL file.
use crate::State;
use rusqlite::{params, Connection, OpenFlags};
use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};
use tracing::info;

pub(crate) async fn snapshot(state: State, directory: PathBuf) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let meta = fs::symlink_metadata(&directory).map_err(|e| e.to_string())?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err("Backup destination must be a real private directory".into());
        }
        let canonical = fs::canonicalize(&directory).map_err(|e| e.to_string())?;
        if canonical.starts_with("/www/wwwroot") {
            return Err("Refusing to save a SQLite backup inside public aaPanel webroot".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?.as_secs();
        let nonce = uuid::Uuid::new_v4();
        let temp = directory.join(format!(".snapshot-{nonce}.tmp"));
        let backup = directory.join(format!("synchro-{timestamp}-{nonce}.sqlite3"));
        let result = (|| -> Result<(), String> {
            {
                let db = state.db.lock().map_err(|_| "SQLite mutex poisoned".to_owned())?;
                // WAL-safe, consistent snapshot, not a raw copy of db/-wal/-shm.
                db.execute("VACUUM INTO ?1", params![temp.to_string_lossy().as_ref()])
                    .map_err(|e| e.to_string())?;
            }
            let verified = Connection::open_with_flags(&temp, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| e.to_string())?;
            let check: String = verified.query_row("PRAGMA integrity_check", [], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            if check != "ok" { return Err("Backup integrity_check failed".into()); }
            drop(verified);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))
                    .map_err(|e| e.to_string())?;
            }
            fs::rename(&temp, &backup).map_err(|e| e.to_string())?;
            info!("SQLite backup: {}", backup.display());
            // Keep seven daily snapshots. For disaster recovery replicate off VPS.
            let mut copies = fs::read_dir(&directory).map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .filter(|entry| {
                    let name = entry.file_name();
                    let n = name.to_string_lossy();
                    n.starts_with("synchro-") && n.ends_with(".sqlite3") &&
                    entry.file_type().map(|t| t.is_file()).unwrap_or(false)
                }).collect::<Vec<_>>();
            copies.sort_by_key(|e| e.file_name());
            let excess = copies.len().saturating_sub(7);
            for entry in copies.into_iter().take(excess) {
                fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
            }
            Ok(())
        })();
        if result.is_err() { let _ = fs::remove_file(&temp); }
        result
    }).await.map_err(|e| e.to_string())?
}
