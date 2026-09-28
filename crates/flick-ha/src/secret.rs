//! Secret storage abstraction for HA tokens.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::HaError;

const KEYRING_SERVICE: &str = "app.flick.desktop";

/// Storage for HA tokens and future OAuth refresh tokens.
pub trait SecretStore: Send + Sync {
    /// Stores a secret under an account key such as `ha:<uuid>`.
    fn set(&self, account: &str, secret: &str) -> Result<(), HaError>;
    /// Reads a secret by account key.
    fn get(&self, account: &str) -> Result<Option<String>, HaError>;
    /// Deletes a secret if it exists.
    fn delete(&self, account: &str) -> Result<(), HaError>;
}

/// In-memory store for tests. It never touches the macOS keychain.
#[derive(Debug, Clone, Default)]
pub struct MemorySecretStore {
    secrets: Arc<Mutex<HashMap<String, String>>>,
}

impl MemorySecretStore {
    /// Creates an empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecretStore {
    fn set(&self, account: &str, secret: &str) -> Result<(), HaError> {
        let mut secrets = self
            .secrets
            .lock()
            .map_err(|err| HaError::Keyring(err.to_string()))?;
        secrets.insert(account.to_owned(), secret.to_owned());
        Ok(())
    }

    fn get(&self, account: &str) -> Result<Option<String>, HaError> {
        let secrets = self
            .secrets
            .lock()
            .map_err(|err| HaError::Keyring(err.to_string()))?;
        Ok(secrets.get(account).cloned())
    }

    fn delete(&self, account: &str) -> Result<(), HaError> {
        let mut secrets = self
            .secrets
            .lock()
            .map_err(|err| HaError::Keyring(err.to_string()))?;
        secrets.remove(account);
        Ok(())
    }
}

/// OS keychain-backed store. Tests must use [`MemorySecretStore`] instead.
#[derive(Debug, Clone, Default)]
pub struct KeyringSecretStore;

impl KeyringSecretStore {
    /// Creates a keychain-backed store using service `app.flick.desktop`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl SecretStore for KeyringSecretStore {
    fn set(&self, account: &str, secret: &str) -> Result<(), HaError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|err| HaError::Keyring(err.to_string()))?;
        entry
            .set_password(secret)
            .map_err(|err| HaError::Keyring(err.to_string()))
    }

    fn get(&self, account: &str) -> Result<Option<String>, HaError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|err| HaError::Keyring(err.to_string()))?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(HaError::Keyring(err.to_string())),
        }
    }

    fn delete(&self, account: &str) -> Result<(), HaError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|err| HaError::Keyring(err.to_string()))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(HaError::Keyring(err.to_string())),
        }
    }
}
