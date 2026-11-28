//! Credential storage and the environment boundary for child processes.

use std::process::Command;

pub const CURSEFORGE_API_KEY: &str = "CURSEFORGE_API_KEY";

/// Launcher credentials are not inherited by child commands.
pub fn sanitize_child_environment(command: &mut Command) -> &mut Command {
    command.env_remove(CURSEFORGE_API_KEY)
}

/// Stores a refresh token under an opaque launcher-generated key.
pub trait CredentialStore {
    fn read(&self, key: &str) -> Result<Option<String>, CredentialError>;
    fn write(&self, key: &str, secret: &str) -> Result<(), CredentialError>;
    fn remove(&self, key: &str) -> Result<(), CredentialError>;
}

/// The system credential store could not complete the requested operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialError;
