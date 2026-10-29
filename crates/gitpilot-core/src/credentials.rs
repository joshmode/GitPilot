//! OS-native credential storage. Tokens are never written to SQLite or config.

use crate::error::{GitPilotError, Result};

const SERVICE: &str = "com.gitpilot.desktop";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    GitHub,
    Provider,
}

impl CredentialKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::GitHub => "github:",
            Self::Provider => "provider:",
        }
    }
}

pub trait CredentialStore: Send + Sync {
    fn set(&self, kind: CredentialKind, id: &str, secret: &str) -> Result<()>;
    fn get(&self, kind: CredentialKind, id: &str) -> Result<Option<String>>;
    fn delete(&self, kind: CredentialKind, id: &str) -> Result<()>;
    fn available(&self) -> bool;
}

fn account_key(kind: CredentialKind, id: &str) -> String {
    format!("{}{}", kind.prefix(), id)
}

/// OS credential storage backed by the platform-native Keychain or Windows
/// Credential Manager. The desktop runtime never selects the keyring crate's
/// in-memory mock backend.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct KeychainStore;

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl KeychainStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(kind: CredentialKind, id: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, &account_key(kind, id))
            .map_err(|error| GitPilotError::Credential(format!("credential entry error: {error}")))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Default for KeychainStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl CredentialStore for KeychainStore {
    fn set(&self, kind: CredentialKind, id: &str, secret: &str) -> Result<()> {
        Self::entry(kind, id)?
            .set_password(secret)
            .map_err(|error| GitPilotError::Credential(format!("credential write failed: {error}")))
    }

    fn get(&self, kind: CredentialKind, id: &str) -> Result<Option<String>> {
        match Self::entry(kind, id)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(GitPilotError::Credential(format!(
                "credential read failed: {error}"
            ))),
        }
    }

    fn delete(&self, kind: CredentialKind, id: &str) -> Result<()> {
        match Self::entry(kind, id)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(GitPilotError::Credential(format!(
                "credential delete failed: {error}"
            ))),
        }
    }

    fn available(&self) -> bool {
        true
    }
}

/// Linux currently has no cached, reviewed Secret Service backend in the
/// reproducible offline build. Refuse storage rather than silently selecting
/// keyring's volatile mock store.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub struct UnsupportedStore;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl UnsupportedStore {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl Default for UnsupportedStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl CredentialStore for UnsupportedStore {
    fn set(&self, _kind: CredentialKind, _id: &str, _secret: &str) -> Result<()> {
        Err(GitPilotError::Unsupported(
            "secure credential storage on Linux requires a reviewed Secret Service backend".into(),
        ))
    }

    fn get(&self, _kind: CredentialKind, _id: &str) -> Result<Option<String>> {
        Err(GitPilotError::Unsupported(
            "secure credential storage on Linux requires a reviewed Secret Service backend".into(),
        ))
    }

    fn delete(&self, _kind: CredentialKind, _id: &str) -> Result<()> {
        Ok(())
    }

    fn available(&self) -> bool {
        false
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn platform_store() -> std::sync::Arc<dyn CredentialStore> {
    std::sync::Arc::new(KeychainStore::new())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn platform_store() -> std::sync::Arc<dyn CredentialStore> {
    std::sync::Arc::new(UnsupportedStore::new())
}

/// In-memory credential store for deterministic tests and demo harnesses. It
/// must never be selected by the desktop runtime.
pub struct MemoryStore {
    values: parking_lot::Mutex<std::collections::HashMap<String, String>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            values: parking_lot::Mutex::new(Default::default()),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialStore for MemoryStore {
    fn set(&self, kind: CredentialKind, id: &str, secret: &str) -> Result<()> {
        self.values
            .lock()
            .insert(account_key(kind, id), secret.to_owned());
        Ok(())
    }

    fn get(&self, kind: CredentialKind, id: &str) -> Result<Option<String>> {
        Ok(self.values.lock().get(&account_key(kind, id)).cloned())
    }

    fn delete(&self, kind: CredentialKind, id: &str) -> Result<()> {
        self.values.lock().remove(&account_key(kind, id));
        Ok(())
    }

    fn available(&self) -> bool {
        true
    }
}
