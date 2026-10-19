//! Optional directory watcher, using std only (no filesystem driver or extra dependency).
//! Only emits new or modified files after their size and mtime are stable twice.
//! Existing files are baselined at startup to avoid resending old builds.
use crate::Event;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc::Sender, Arc};
use std::time::{Duration, SystemTime};

type Signature = (u64, SystemTime);

#[derive(Default)]
pub struct FolderWatch {
    config: Option<(bool, String)>,
    stop: Option<Arc<AtomicBool>>,
}

fn eligible(path: &Path, size: u64) -> bool {
    if size == 0 || size > crate::MAX_SIZE {
        return false;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    // This watcher is intended for Android build outputs, not arbitrary private files.
    !name.starts_with('.') && name.to_ascii_lowercase().ends_with(".apk")
}

fn scan(dir: &Path) -> HashMap<PathBuf, Signature> {
    let mut found = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return found };
    for entry in entries.flatten() {
        let path = entry.path();
        // Symlinks/reparse-file links must not escape the user's selected output folder.
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if meta.file_type().is_symlink() { continue; }
        if !meta.is_file() || !eligible(&path, meta.len()) { continue; }
        let stamp = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        found.insert(path, (meta.len(), stamp));
    }
    found
}

impl FolderWatch {
    pub fn configure(&mut self, enabled: bool, path: &str, tx: Sender<Event>) {
        let next = (enabled, path.trim().to_owned());
        if self.config.as_ref() == Some(&next) {
            return;
        }
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::SeqCst);
        }
        self.config = Some(next.clone());
        if !next.0 || !Path::new(&next.1).is_dir() {
            return;
        }
        let flag = Arc::new(AtomicBool::new(false));
        self.stop = Some(flag.clone());
        std::thread::spawn(move || {
            let folder = PathBuf::from(next.1);
            let mut previous = scan(&folder);
            let mut sent = previous.clone();
            while !flag.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1200));
                if flag.load(Ordering::SeqCst) { break; }
                let current = scan(&folder);
                for (path, sig) in &current {
                    let settled = SystemTime::now().duration_since(sig.1)
                        .map(|age| age >= Duration::from_secs(8)).unwrap_or(false);
                    if settled && previous.get(path) == Some(sig) && sent.get(path) != Some(sig) {
                        let _ = tx.send(Event::WatchFile(path.clone()));
                        sent.insert(path.clone(), *sig);
                    }
                }
                previous = current;
            }
        });
    }
}

impl Drop for FolderWatch {
    fn drop(&mut self) {
        if let Some(flag) = self.stop.take() {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ignores_transient_and_empty_files() {
        assert!(!eligible(Path::new("build.apk.part"), 512));
        assert!(!eligible(Path::new("private.txt"), 512));
        assert!(!eligible(Path::new("build.zip"), 512));
        assert!(eligible(Path::new("BUILD.APK"), 512));
        assert!(!eligible(Path::new("build.apk.tmp"), 512));
        assert!(!eligible(Path::new(".hidden.apk"), 512));
        assert!(!eligible(Path::new("build.apk"), 0));
        assert!(eligible(Path::new("build.apk"), 1024));
    }
}
