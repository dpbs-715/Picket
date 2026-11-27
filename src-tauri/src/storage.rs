//! Unified private local storage foundation for `~/.picket/` (`0700` dir, `0600` atomic files).
//!
//! Guarantees:
//! - Strict `0700` directory permissions and `0600` atomic temp-file + `sync_all` + `persist`.
//! - Process-wide per-path mutex serialization shared across synchronous and async (`spawn_blocking`)
//!   callers so concurrent read-modify-write operations never silently clobber one another.
//! - Blocking file I/O and `fsync` run on Tokio's blocking pool (`run_blocking_locked`) when invoked
//!   from async coordinators.
//! - Corrupted or incompatible JSON files are preserved via bounded `0600` `.corrupt.<ts>.bak`
//!   copies instead of being silently lost on subsequent saves.

use crate::time;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use tempfile::NamedTempFile;

const MAX_CORRUPT_BACKUPS_PER_FILE: usize = 3;

static PATH_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();

fn lock_for_path(path: &Path) -> Result<Arc<Mutex<()>>, String> {
    let key = normalize_lock_key(path);
    let registry = PATH_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = registry
        .lock()
        .map_err(|_| "Storage path lock registry is unavailable".to_string())?;
    Ok(Arc::clone(guard.entry(key).or_default()))
}

fn normalize_lock_key(path: &Path) -> PathBuf {
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name())
        && let Ok(canon_parent) = fs::canonicalize(parent)
    {
        return canon_parent.join(name);
    }
    path.to_path_buf()
}

pub fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir)
        .map_err(|err| format!("Failed to create Picket storage directory: {err}"))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .map_err(|err| format!("Failed to set 0700 permissions on Picket storage directory: {err}"))
}

pub fn write_bytes_atomic(dir: &Path, target: &Path, bytes: &[u8]) -> Result<(), String> {
    ensure_private_dir(dir)?;
    let mut temp = NamedTempFile::new_in(dir)
        .map_err(|err| format!("Failed to create temporary storage file: {err}"))?;
    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|err| {
            format!("Failed to set 0600 permissions on temporary storage file: {err}")
        })?;
    temp.write_all(bytes)
        .map_err(|err| format!("Failed to write temporary storage file: {err}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|err| format!("Failed to sync storage file to disk: {err}"))?;
    temp.persist(target)
        .map_err(|err| format!("Failed to atomically persist storage file: {err}"))?;
    Ok(())
}

pub fn write_json_atomic<T: Serialize>(
    dir: &Path,
    target: &Path,
    value: &T,
    trailing_newline: bool,
) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|err| format!("Failed to serialize persisted JSON: {err}"))?;
    if trailing_newline {
        bytes.push(b'\n');
    }
    write_bytes_atomic(dir, target, &bytes)
}

/// Serializes synchronous operations on `path` across all callers in this process.
pub fn with_file_lock<T>(path: &Path, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let lock = lock_for_path(path)?;
    let _guard = lock
        .lock()
        .map_err(|_| "Storage file lock is unavailable".to_string())?;
    f()
}

/// Runs a blocking file operation off the Tokio worker pool while holding the per-path lock.
pub async fn run_blocking_locked<T, F>(path: PathBuf, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tokio::task::spawn_blocking(move || with_file_lock(&path, f))
        .await
        .map_err(|err| format!("Storage worker task failed: {err}"))?
}

/// Preserves a `0600` backup copy (`<file>.corrupt.<YYYYMMDDTHHMMSSZ>.bak`) of an unreadable or
/// unparseable persisted file without modifying the original file in place.
///
/// Deduplicates identical content and retains at most `MAX_CORRUPT_BACKUPS_PER_FILE` backups.
pub fn backup_corrupt_copy(path: &Path, raw_bytes: &[u8]) -> Option<PathBuf> {
    if raw_bytes.is_empty() {
        return None;
    }
    let dir = path.parent()?;
    let file_name = path.file_name()?.to_str()?;
    let prefix = format!("{file_name}.corrupt.");

    let mut existing: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|candidate| {
            candidate
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".bak"))
        })
        .collect();
    existing.sort();

    // Avoid creating duplicate backups if the exact same broken payload was already archived.
    for candidate in existing.iter().rev() {
        if fs::read(candidate)
            .ok()
            .is_some_and(|saved| saved == raw_bytes)
        {
            return Some(candidate.clone());
        }
    }

    let stamp = time::now_utc_compact();
    let mut backup_path = dir.join(format!("{prefix}{stamp}.bak"));
    if backup_path.exists() {
        backup_path = dir.join(format!("{prefix}{stamp}-{}.bak", time::now_millis()));
    }
    write_bytes_atomic(dir, &backup_path, raw_bytes).ok()?;

    existing.push(backup_path.clone());
    existing.sort();
    while existing.len() > MAX_CORRUPT_BACKUPS_PER_FILE {
        let oldest = existing.remove(0);
        let _ = fs::remove_file(oldest);
    }
    Some(backup_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn writes_atomic_json_with_strict_0700_and_0600_permissions() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join("picket-private");
        let target = data_dir.join("sample.json");

        write_json_atomic(&data_dir, &target, &serde_json::json!({ "ok": true }), true).unwrap();

        assert_eq!(
            fs::metadata(&data_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let raw = fs::read_to_string(&target).unwrap();
        assert!(raw.ends_with('\n'));
        assert!(raw.contains("\"ok\": true"));
    }

    #[test]
    fn corrupt_backup_preserves_original_deduplicates_and_bounds_count() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("state.json");
        fs::write(&target, b"{corrupt-v1").unwrap();

        let b1 = backup_corrupt_copy(&target, b"{corrupt-v1").unwrap();
        assert_eq!(
            fs::metadata(&b1).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read(&target).unwrap(), b"{corrupt-v1");

        // Identical payload reuses existing backup instead of spamming files:
        let b1_dup = backup_corrupt_copy(&target, b"{corrupt-v1").unwrap();
        assert_eq!(b1, b1_dup);

        // Distinct payloads are capped at MAX_CORRUPT_BACKUPS_PER_FILE:
        for idx in 2..=6 {
            let payload = format!("{{corrupt-v{idx}");
            backup_corrupt_copy(&target, payload.as_bytes()).unwrap();
        }
        let backups: Vec<_> = fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|v| v.file_name().to_string_lossy().into_owned()))
            .filter(|name| name.starts_with("state.json.corrupt.") && name.ends_with(".bak"))
            .collect();
        assert_eq!(backups.len(), MAX_CORRUPT_BACKUPS_PER_FILE);
    }

    #[tokio::test]
    async fn run_blocking_locked_serializes_concurrent_read_modify_writes() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("counter.json");
        write_json_atomic(temp.path(), &target, &0_usize, false).unwrap();

        let completed = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..16 {
            let dir = temp.path().to_path_buf();
            let path = target.clone();
            let completed = Arc::clone(&completed);
            handles.push(tokio::spawn(async move {
                run_blocking_locked(path.clone(), move || {
                    let current: usize =
                        serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                            .map_err(|e| e.to_string())?;
                    write_json_atomic(&dir, &path, &(current + 1), false)?;
                    completed.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .await
                .unwrap();
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }
        assert_eq!(completed.load(Ordering::SeqCst), 16);
        let final_val: usize = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
        assert_eq!(final_val, 16);
    }
}
