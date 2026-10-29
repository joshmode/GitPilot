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

#[cfg(target_os = "macos")]
pub struct KeychainStore;

#[cfg(target_os = "macos")]
impl KeychainStore {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(target_os = "macos")]
impl Default for KeychainStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
impl CredentialStore for KeychainStore {
    fn set(&self, kind: CredentialKind, id: &str, secret: &str) -> Result<()> {
        use security_framework::passwords::set_generic_password;
        set_generic_password(SERVICE, &account_key(kind, id), secret.as_bytes())
            .map_err(|e| GitPilotError::Credential(format!("Keychain write failed ({})", e.code())))
    }

    fn get(&self, kind: CredentialKind, id: &str) -> Result<Option<String>> {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(SERVICE, &account_key(kind, id)) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| GitPilotError::Credential("credential is not UTF-8".into())),
            Err(error) if error.code() == -25300 => Ok(None),
            Err(error) => Err(GitPilotError::Credential(format!(
                "Keychain read failed ({})",
                error.code()
            ))),
        }
    }

    fn delete(&self, kind: CredentialKind, id: &str) -> Result<()> {
        use security_framework::passwords::delete_generic_password;
        match delete_generic_password(SERVICE, &account_key(kind, id)) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25300 => Ok(()),
            Err(error) => Err(GitPilotError::Credential(format!(
                "Keychain delete failed ({})",
                error.code()
            ))),
        }
    }

    fn available(&self) -> bool {
        true
    }
}

/// Deliberately refuses to persist secrets on platforms without a reviewed
/// native credential backend. A plaintext fallback would violate the security
/// contract.
#[cfg(not(target_os = "macos"))]
pub struct UnsupportedStore;

#[cfg(not(target_os = "macos"))]
impl UnsupportedStore {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(target_os = "macos"))]
impl CredentialStore for UnsupportedStore {
    fn set(&self, _kind: CredentialKind, _id: &str, _secret: &str) -> Result<()> {
        Err(GitPilotError::Unsupported(
            "secure credential storage requires a platform adapter".into(),
        ))
    }

    fn get(&self, _kind: CredentialKind, _id: &str) -> Result<Option<String>> {
        Err(GitPilotError::Unsupported(
            "secure credential storage requires a platform adapter".into(),
        ))
    }

    fn delete(&self, _kind: CredentialKind, _id: &str) -> Result<()> {
        Ok(())
    }

    fn available(&self) -> bool {
        false
    }
}

#[cfg(target_os = "macos")]
pub fn platform_store() -> std::sync::Arc<dyn CredentialStore> {
    std::sync::Arc::new(KeychainStore::new())
}

#[cfg(not(target_os = "macos"))]
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
