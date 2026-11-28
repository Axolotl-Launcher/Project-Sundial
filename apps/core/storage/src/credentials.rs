// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Platform credential storage and child process credential isolation.

use std::process::Command;

use axlauncher_spi::credentials::{CredentialError, CredentialStore};

const SERVICE: &str = "AXLauncher Minecraft accounts";
pub const CURSEFORGE_API_KEY: &str = "CURSEFORGE_API_KEY";

/// Launcher credentials are not inherited by child commands.
pub fn sanitize_child_environment(command: &mut Command) -> &mut Command {
    command.env_remove(CURSEFORGE_API_KEY)
}

/// Keeps account tokens in the platform credential store.
#[derive(Clone, Copy, Default)]
pub struct SystemCredentials;

impl CredentialStore for SystemCredentials {
    fn read(&self, key: &str) -> Result<Option<String>, CredentialError> {
        match entry(key)?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(CredentialError),
        }
    }

    fn write(&self, key: &str, secret: &str) -> Result<(), CredentialError> {
        entry(key)?
            .set_password(secret)
            .map_err(|_| CredentialError)
    }

    fn remove(&self, key: &str) -> Result<(), CredentialError> {
        match entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(CredentialError),
        }
    }
}

fn entry(key: &str) -> Result<keyring::Entry, CredentialError> {
    keyring::Entry::new(SERVICE, key).map_err(|_| CredentialError)
}
