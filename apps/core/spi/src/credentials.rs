//! The interface for the platform credential store.

/// Stores a refresh token under an opaque launcher-generated key.
pub trait CredentialStore {
    fn read(&self, key: &str) -> Result<Option<String>, CredentialError>;
    fn write(&self, key: &str, secret: &str) -> Result<(), CredentialError>;
    fn remove(&self, key: &str) -> Result<(), CredentialError>;
}

/// The system credential store could not complete the requested operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialError;
