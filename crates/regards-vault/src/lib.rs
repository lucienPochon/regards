//! Secrets in the operating system's credential store (ADR-0008, ADR-0013).
//!
//! - **Windows:** Credential Manager.
//! - **macOS:** the login Keychain.
//! - **Linux:** the Secret Service (GNOME Keyring, KWallet).
//!
//! Secrets never go into the Regards database or logs; the database stores
//! only the [`SecretKey`] that names one. Credential-store calls can block
//! (a Keychain prompt, a D-Bus round trip), so every call runs on a blocking
//! thread.

use keyring_core::{CredentialStore, Entry};
pub use regards_core::secret::Secret;
use std::fmt;
use std::sync::Arc;

/// Service name under which Regards keeps its credentials.
const SERVICE: &str = "Regards";

/// Errors from the credential store.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("the system credential store is not available: {0}")]
    Unavailable(String),
    #[error("the system credential store refused the request: {0}")]
    Store(String),
    #[error("a stored secret is not valid text")]
    BadEncoding,
}

/// What a secret is for, per account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// The password or app password used for IMAP and SMTP.
    MailPassword,
    /// The OAuth refresh token that gets access tokens for IMAP and SMTP.
    OauthRefresh,
}

impl SecretKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::MailPassword => "mail-password",
            Self::OauthRefresh => "oauth-refresh",
        }
    }
}

/// The name of one secret. This, not the secret, is what the database stores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretKey(String);

impl SecretKey {
    pub fn new(account_id: &str, kind: SecretKind) -> Self {
        Self(format!("{account_id}/{}", kind.as_str()))
    }

    /// Restore a key read back from the database.
    pub fn from_stored(stored: impl Into<String>) -> Self {
        Self(stored.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Access to one credential store.
#[derive(Clone)]
pub struct Vault {
    store: Arc<CredentialStore>,
}

impl fmt::Debug for Vault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vault").field("store", &self.store.vendor()).finish()
    }
}

impl Vault {
    /// The platform's credential store.
    pub fn native() -> Result<Self, VaultError> {
        Ok(Self {
            store: native_store().map_err(|error| VaultError::Unavailable(error.to_string()))?,
        })
    }

    /// A store that lives only in this process, for tests.
    pub fn in_memory() -> Self {
        Self {
            store: keyring_core::mock::Store::new().expect("the mock store cannot fail"),
        }
    }

    pub async fn set(&self, key: &SecretKey, secret: &Secret) -> Result<(), VaultError> {
        let secret = secret.clone();
        self.with_entry(key, move |entry| {
            entry.set_password(secret.expose()).map_err(store_error)
        })
        .await
    }

    /// The secret, or `None` when there is none under `key`.
    pub async fn get(&self, key: &SecretKey) -> Result<Option<Secret>, VaultError> {
        self.with_entry(key, |entry| match entry.get_password() {
            Ok(value) => Ok(Some(Secret::new(value))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(keyring_core::Error::BadEncoding(_)) => Err(VaultError::BadEncoding),
            Err(error) => Err(store_error(error)),
        })
        .await
    }

    /// Remove a secret. Removing one that does not exist is not an error.
    pub async fn delete(&self, key: &SecretKey) -> Result<(), VaultError> {
        self.with_entry(key, |entry| match entry.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(store_error(error)),
        })
        .await
    }

    async fn with_entry<T: Send + 'static>(
        &self,
        key: &SecretKey,
        op: impl FnOnce(&Entry) -> Result<T, VaultError> + Send + 'static,
    ) -> Result<T, VaultError> {
        let store = Arc::clone(&self.store);
        let user = key.0.clone();
        tokio::task::spawn_blocking(move || {
            let entry = store.build(SERVICE, &user, None).map_err(store_error)?;
            op(&entry)
        })
        .await
        .map_err(|error| VaultError::Store(format!("credential store task failed: {error}")))?
    }
}

fn store_error(error: keyring_core::Error) -> VaultError {
    match error {
        keyring_core::Error::NoStorageAccess(inner) => VaultError::Unavailable(inner.to_string()),
        other => VaultError::Store(other.to_string()),
    }
}

#[cfg(windows)]
fn native_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(windows_native_keyring_store::Store::new()?)
}

#[cfg(target_os = "macos")]
fn native_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(apple_native_keyring_store::keychain::Store::new()?)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn native_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(zbus_secret_service_keyring_store::Store::new()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn set_get_and_delete_round_trip() {
        let vault = Vault::in_memory();
        let key = SecretKey::new("acct-1", SecretKind::MailPassword);
        assert!(vault.get(&key).await.unwrap().is_none());

        vault.set(&key, &Secret::new("hunter2")).await.unwrap();
        assert_eq!(vault.get(&key).await.unwrap().unwrap().expose(), "hunter2");

        vault.set(&key, &Secret::new("changed")).await.unwrap();
        assert_eq!(vault.get(&key).await.unwrap().unwrap().expose(), "changed");

        vault.delete(&key).await.unwrap();
        assert!(vault.get(&key).await.unwrap().is_none());
        vault.delete(&key).await.unwrap();
    }

    #[test]
    fn keys_and_debug_output_never_show_the_secret() {
        assert_eq!(
            SecretKey::new("abc", SecretKind::MailPassword).as_str(),
            "abc/mail-password"
        );
        assert_eq!(format!("{:?}", Secret::new("hunter2")), "Secret(<redacted>)");
    }

    /// Writes to the real credential store, so it is opt-in:
    /// `cargo test -p regards-vault -- --ignored`.
    #[tokio::test]
    #[ignore = "uses the real OS credential store"]
    async fn native_store_round_trip() {
        let vault = Vault::native().unwrap();
        let key = SecretKey::new("regards-test-0000", SecretKind::MailPassword);
        vault.set(&key, &Secret::new("native-test")).await.unwrap();
        assert_eq!(vault.get(&key).await.unwrap().unwrap().expose(), "native-test");
        vault.delete(&key).await.unwrap();
        assert!(vault.get(&key).await.unwrap().is_none());
    }
}
