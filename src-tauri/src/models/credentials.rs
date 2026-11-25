//! Local file-backed credential storage (`~/.picket/credentials.json`, mode `0600`
//! inside `0700` storage directory).
//! Never stores credentials in `models.json`, logs, or ordinary IPC payloads.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tempfile::NamedTempFile;

const CREDENTIALS_VERSION: u8 = 1;
pub const CREDENTIALS_FILE: &str = "credentials.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    io_lock: Arc<Mutex<()>>,
    read_calls: Arc<AtomicU64>,
}

impl CredentialStore {
    #[cfg(test)]
    pub fn read_count(&self) -> u64 {
        self.read_calls.load(Ordering::SeqCst)
    }

    fn read_file(&self, storage_dir: &Path) -> Result<StoredCredentialsFile, String> {
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
            _ => Err("Local credentials file (~/.picket/credentials.json) is corrupted".into()),
        }
    }

    fn write_file_atomic(
        &self,
        storage_dir: &Path,
        payload: &StoredCredentialsFile,
    ) -> Result<(), String> {
        fs::create_dir_all(storage_dir)
            .map_err(|_| "Cannot create local configuration directory".to_string())?;
        fs::set_permissions(storage_dir, fs::Permissions::from_mode(0o700))
            .map_err(|_| "Cannot set local configuration directory permissions".to_string())?;
        let serialized = serde_json::to_vec_pretty(payload)
            .map_err(|_| "Cannot serialize local credentials".to_string())?;
        let mut temp = NamedTempFile::new_in(storage_dir)
            .map_err(|_| "Cannot create temporary credentials file".to_string())?;
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o600))
            .map_err(|_| "Cannot set credentials file permissions".to_string())?;
        temp.write_all(&serialized)
            .map_err(|_| "Cannot write local credentials file".to_string())?;
        temp.as_file()
            .sync_all()
            .map_err(|_| "Cannot sync local credentials file".to_string())?;
        temp.persist(storage_dir.join(CREDENTIALS_FILE))
            .map_err(|_| "Cannot atomically persist local credentials file".to_string())?;
        Ok(())
    }

    pub fn put(&self, storage_dir: &Path, connection_id: &str, secret: &str) -> Result<(), String> {
        let _guard = self
            .io_lock
            .lock()
            .map_err(|_| "Credential store lock is unavailable")?;
        let mut file = self.read_file(storage_dir)?;
        file.keys
            .insert(connection_id.to_owned(), secret.to_owned());
        self.write_file_atomic(storage_dir, &file)
    }

    pub fn get(&self, storage_dir: &Path, connection_id: &str) -> Result<Option<String>, String> {
        self.read_calls.fetch_add(1, Ordering::SeqCst);
        let _guard = self
            .io_lock
            .lock()
            .map_err(|_| "Credential store lock is unavailable")?;
        let file = self.read_file(storage_dir)?;
        Ok(file.keys.get(connection_id).cloned())
    }

    pub fn delete(&self, storage_dir: &Path, connection_id: &str) -> Result<(), String> {
        let _guard = self
            .io_lock
            .lock()
            .map_err(|_| "Credential store lock is unavailable")?;
        let path = storage_dir.join(CREDENTIALS_FILE);
        if !path.exists() {
            return Ok(());
        }
        let mut file = self.read_file(storage_dir)?;
        if file.keys.remove(connection_id).is_some() {
            self.write_file_atomic(storage_dir, &file)?;
        }
        Ok(())
    }
}
