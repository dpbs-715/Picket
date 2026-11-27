//! Persistent non-secret model catalog (`~/.picket/models.json` v3: provider connections
//! and child models) paired with `~/.picket/credentials.json` (`CredentialStore`),
//! macOS `LocalAuthentication` for on-demand key reveal, and Pi Core task execution.

mod catalog;
mod connection;
mod persistence;

pub use persistence::ResolvedRuntimeSpec;

use super::{
    credentials::CredentialStore,
    local_auth::{LocalAuthenticator, MacOsLocalAuthenticator},
    types::ProviderKind,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, atomic::AtomicU64},
};
use tokio::sync::oneshot;

#[derive(Clone)]
pub struct ModelStore {
    pub(super) credentials: CredentialStore,
    pub(super) authenticator: Arc<Mutex<Arc<dyn LocalAuthenticator>>>,
    pub(super) gate: Arc<tokio::sync::Mutex<()>>,
    pub(super) counter: Arc<AtomicU64>,
    pub(super) endpoint_overrides: Arc<Mutex<HashMap<ProviderKind, String>>>,
    pub(super) active_list_cancels: Arc<Mutex<HashMap<String, oneshot::Sender<()>>>>,
}

impl Default for ModelStore {
    fn default() -> Self {
        Self::with_credentials_and_authenticator(
            CredentialStore::default(),
            Arc::new(MacOsLocalAuthenticator),
        )
    }
}

impl ModelStore {
    pub fn with_credentials_and_authenticator(
        credentials: CredentialStore,
        authenticator: Arc<dyn LocalAuthenticator>,
    ) -> Self {
        Self {
            credentials,
            authenticator: Arc::new(Mutex::new(authenticator)),
            gate: Arc::new(tokio::sync::Mutex::new(())),
            counter: Arc::new(AtomicU64::new(0)),
            endpoint_overrides: Arc::new(Mutex::new(HashMap::new())),
            active_list_cancels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn cancel_list_request(&self, request_id: &str) -> bool {
        let sender = self
            .active_list_cancels
            .lock()
            .ok()
            .and_then(|mut map| map.remove(request_id));
        if let Some(tx) = sender {
            let _ = tx.send(());
            true
        } else {
            false
        }
    }

    pub fn cancel_all_list_requests(&self) {
        let senders: Vec<oneshot::Sender<()>> = self
            .active_list_cancels
            .lock()
            .map(|mut map| map.drain().map(|(_, tx)| tx).collect())
            .unwrap_or_default();
        for tx in senders {
            let _ = tx.send(());
        }
    }

    #[cfg(test)]
    pub fn credentials_store(&self) -> &CredentialStore {
        &self.credentials
    }

    #[cfg(feature = "smoke-harness")]
    pub fn set_authenticator(
        &self,
        authenticator: Arc<dyn LocalAuthenticator>,
    ) -> Result<(), String> {
        *self
            .authenticator
            .lock()
            .map_err(|_| "Authenticator lock is unavailable".to_string())? = authenticator;
        Ok(())
    }

    pub(super) fn current_authenticator(&self) -> Result<Arc<dyn LocalAuthenticator>, String> {
        self.authenticator
            .lock()
            .map(|guard| Arc::clone(&*guard))
            .map_err(|_| "Authenticator lock is unavailable".to_string())
    }

    #[cfg(test)]
    pub fn set_endpoint_override(&self, provider: ProviderKind, base_url: String) {
        self.endpoint_overrides
            .lock()
            .unwrap()
            .insert(provider, base_url);
    }

    pub(super) fn effective_base_url(
        &self,
        provider: ProviderKind,
        configured: Option<&str>,
    ) -> String {
        if let Some(url) = configured.filter(|s| !s.trim().is_empty()) {
            return url.trim().to_owned();
        }
        if let Ok(overrides) = self.endpoint_overrides.lock()
            && let Some(override_url) = overrides.get(&provider)
        {
            return override_url.clone();
        }
        provider.default_base_url().to_owned()
    }
}
