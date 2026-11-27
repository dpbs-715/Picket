//! Local file-backed credential storage (`~/.picket/credentials.json`, mode `0600`
//! inside `0700` storage directory).
//! Never stores credentials in `models.json`, logs, or ordinary IPC payloads.
use crate::storage;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

const CREDENTIALS_VERSION: u8 = 1;
pub const CREDENTIALS_FILE: &str = "credentials.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredCredentialsFile {
    version: u8,
    keys: BTreeMap<String, String>,
}

impl StoredCredentialsFile {
    fn empty() -> Self {
        Self {
            version: CREDENTIALS_VERSION,
            keys: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Default)]
pub struct CredentialStore {
    read_calls: Arc<AtomicU64>,
}

impl CredentialStore {
    #[cfg(test)]
    pub fn read_count(&self) -> u64 {
        self.read_calls.load(Ordering::SeqCst)
    }

    fn read_file_unlocked(storage_dir: &Path) -> Result<StoredCredentialsFile, String> {
        let path = storage_dir.join(CREDENTIALS_FILE);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(StoredCredentialsFile::empty());
            }
            Err(_) => {
                return Err(
                    "Cannot read local credentials file (~/.picket/credentials.json)".into(),
                );
            }
        };
        match serde_json::from_slice::<StoredCredentialsFile>(&bytes) {
            Ok(parsed) if parsed.version == CREDENTIALS_VERSION => Ok(parsed),
            _ => {
                storage::backup_corrupt_copy(&path, &bytes);
                Err("Local credentials file (~/.picket/credentials.json) is corrupted".into())
            }
        }
    }

    fn write_file_atomic_unlocked(
        storage_dir: &Path,
        payload: &StoredCredentialsFile,
    ) -> Result<(), String> {
        storage::write_json_atomic(
            storage_dir,
            &storage_dir.join(CREDENTIALS_FILE),
            payload,
            false,
        )
    }

    pub async fn put_async(
        &self,
        storage_dir: &Path,
        connection_id: &str,
        secret: &str,
    ) -> Result<(), String> {
        let dir = storage_dir.to_path_buf();
        let path = dir.join(CREDENTIALS_FILE);
        let conn_id = connection_id.to_owned();
        let secret_owned = secret.to_owned();
        storage::run_blocking_locked(path, move || {
            let mut file = Self::read_file_unlocked(&dir)?;
            file.keys.insert(conn_id, secret_owned);
            Self::write_file_atomic_unlocked(&dir, &file)
        })
        .await
    }

    #[cfg(test)]
    pub fn get(&self, storage_dir: &Path, connection_id: &str) -> Result<Option<String>, String> {
        self.read_calls.fetch_add(1, Ordering::SeqCst);
        let path = storage_dir.join(CREDENTIALS_FILE);
        storage::with_file_lock(&path, || {
            let file = Self::read_file_unlocked(storage_dir)?;
            Ok(file.keys.get(connection_id).cloned())
        })
    }

    pub async fn get_async(
        &self,
        storage_dir: &Path,
        connection_id: &str,
    ) -> Result<Option<String>, String> {
        self.read_calls.fetch_add(1, Ordering::SeqCst);
        let dir = storage_dir.to_path_buf();
        let path = dir.join(CREDENTIALS_FILE);
        let conn_id = connection_id.to_owned();
        storage::run_blocking_locked(path, move || {
            let file = Self::read_file_unlocked(&dir)?;
            Ok(file.keys.get(&conn_id).cloned())
        })
        .await
    }

    pub async fn delete_async(
        &self,
        storage_dir: &Path,
        connection_id: &str,
    ) -> Result<(), String> {
        let dir = storage_dir.to_path_buf();
        let path = dir.join(CREDENTIALS_FILE);
        let conn_id = connection_id.to_owned();
        storage::run_blocking_locked(path.clone(), move || {
            if !path.exists() {
                return Ok(());
            }
            let mut file = Self::read_file_unlocked(&dir)?;
            if file.keys.remove(&conn_id).is_some() {
                Self::write_file_atomic_unlocked(&dir, &file)?;
            }
            Ok(())
        })
        .await
    }
}
