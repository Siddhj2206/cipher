//! Where API keys and tokens live: the OS keychain, never a Book or a config file.

use std::collections::HashMap;
use std::sync::Mutex;

/// The keychain could not be read or written.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("secret store error: {0}")]
pub struct SecretsError(pub String);

/// A store of secrets by string id. Missing ids are not errors.
pub trait Secrets: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<String>, SecretsError>;
    fn set(&self, id: &str, secret: &str) -> Result<(), SecretsError>;
    /// Deleting an id that is not there succeeds.
    fn delete(&self, id: &str) -> Result<(), SecretsError>;
}

/// The OS keychain (Secret Service on Linux), under the service name `cipher`.
#[derive(Debug, Default, Clone, Copy)]
pub struct Keychain;

impl Keychain {
    const SERVICE: &'static str = "cipher";

    fn entry(id: &str) -> Result<keyring::Entry, SecretsError> {
        keyring::Entry::new(Self::SERVICE, id).map_err(|e| SecretsError(e.to_string()))
    }
}

impl Secrets for Keychain {
    fn get(&self, id: &str) -> Result<Option<String>, SecretsError> {
        match Self::entry(id)?.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(SecretsError(e.to_string())),
        }
    }

    fn set(&self, id: &str, secret: &str) -> Result<(), SecretsError> {
        Self::entry(id)?
            .set_password(secret)
            .map_err(|e| SecretsError(e.to_string()))
    }

    fn delete(&self, id: &str) -> Result<(), SecretsError> {
        match Self::entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(SecretsError(e.to_string())),
        }
    }
}

/// Secrets in memory, for tests.
#[derive(Debug, Default)]
pub struct InMemory {
    map: Mutex<HashMap<String, String>>,
}

impl InMemory {
    pub fn new() -> Self {
        Self::default()
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Secrets for InMemory {
    fn get(&self, id: &str) -> Result<Option<String>, SecretsError> {
        Ok(self.map().get(id).cloned())
    }

    fn set(&self, id: &str, secret: &str) -> Result<(), SecretsError> {
        self.map().insert(id.to_owned(), secret.to_owned());
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), SecretsError> {
        self.map().remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_round_trip() {
        let s = InMemory::new();
        assert_eq!(s.get("a").unwrap(), None);
        s.set("a", "one").unwrap();
        s.set("a", "two").unwrap();
        assert_eq!(s.get("a").unwrap().as_deref(), Some("two"));
        s.delete("a").unwrap();
        s.delete("a").unwrap();
        assert_eq!(s.get("a").unwrap(), None);
    }

    /// Touches the real OS keychain, so it only runs on request:
    /// `cargo test -p cipher-llm -- --ignored keychain`.
    #[test]
    #[ignore = "uses the real OS keychain"]
    fn keychain_round_trip() {
        let k = Keychain;
        let id = "cipher-llm-test-entry";
        k.set(id, "not-a-real-secret").unwrap();
        assert_eq!(k.get(id).unwrap().as_deref(), Some("not-a-real-secret"));
        k.delete(id).unwrap();
        assert_eq!(k.get(id).unwrap(), None);
    }
}
