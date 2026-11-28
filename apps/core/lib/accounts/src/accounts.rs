// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Minecraft account identity and a versioned account document.

use std::io;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

mod document;
mod hierarchy;
pub mod microsoft;
mod sessions;
mod sign_in;
pub use sign_in::SignInCommit;
pub mod yggdrasil;
pub use axlauncher_worker::accounts::AccountId;
use axlauncher_worker::accounts::{AccountKey, LauncherId};
pub use sessions::{AccountService, LaunchIdentity};

/// A saved Minecraft identity with its local credential reference.
#[derive(Clone, Serialize, Deserialize)]
pub struct McAccount {
    id: AccountId,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credential: Option<CredentialRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    server: Option<axlauncher_worker::accounts::Server>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl McAccount {
    /// An offline identity with Minecraft's name-based UUID as the default.
    pub fn offline(name: &str, uuid: Option<&str>) -> Result<Self, AccountError> {
        let name = name.trim();
        if !(1..=16).contains(&name.chars().count())
            || !name
                .chars()
                .all(|character| character.is_alphanumeric() || character == '_')
        {
            return Err(AccountError::InvalidName);
        }
        let uuid = match uuid.map(str::trim).filter(|value| !value.is_empty()) {
            Some(uuid) => Uuid::parse_str(uuid).map_err(|_| AccountError::InvalidUuid)?,
            None => offline_uuid(name),
        };
        Ok(Self {
            id: AccountId::Offline(uuid),
            name: name.to_owned(),
            credential: None,
            server: None,
            extra: Map::new(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn server(&self) -> Option<&axlauncher_worker::accounts::Server> {
        self.server.as_ref()
    }

    fn has_expected_provider(&self) -> bool {
        match self.id {
            AccountId::Yggdrasil { .. } => self
                .server()
                .is_some_and(|server| yggdrasil::account_id(server, self.id.uuid()) == self.id),
            AccountId::Microsoft(_) => self.server.is_none(),
            AccountId::Offline(_) => self.server.is_none() && self.credential.is_none(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
enum CredentialRef {
    Legacy(Uuid),
    Scoped { key: Uuid },
}

fn offline_uuid(name: &str) -> Uuid {
    let digest = Md5::digest(format!("OfflinePlayer:{name}"));
    uuid::Builder::from_md5_bytes(digest.into()).into_uuid()
}

/// Why an account document or an identity could not be used.
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("Sign-in was cancelled.")]
    Cancelled,
    #[error("Use 1–16 letters, numbers, or underscores for the player name.")]
    InvalidName,
    #[error("Enter a valid Minecraft UUID.")]
    InvalidUuid,
    #[error("An offline account with this UUID already exists.")]
    Duplicate,
    #[error("This account is no longer available.")]
    Missing,
    #[error("This launcher account is no longer available.")]
    MissingLauncher,
    #[error("Enter a launcher account name of 1–48 characters without line breaks or tabs.")]
    InvalidLauncherName,
    #[error("A launcher account already uses this name.")]
    DuplicateLauncherName,
    #[error("Could not identify the current device user.")]
    DeviceUser,
    #[error("The account file uses unsupported schema {0}.")]
    UnsupportedSchema(u32),
    #[error("Could not read the account file: {0}")]
    Read(#[source] io::Error),
    #[error("The account file is unreadable.")]
    Decode,
    #[error("The account file contains an invalid Minecraft account provider.")]
    InvalidServer,
    #[error("The account file contains invalid or duplicate account references.")]
    InvalidHierarchy,
    #[error("Could not save the account file: {0}")]
    Write(#[source] io::Error),
    #[error("Could not access the system credential store. Unlock it or sign in again.")]
    CredentialStore,
    #[error(transparent)]
    Microsoft(#[from] microsoft::MicrosoftError),
    #[error(transparent)]
    Yggdrasil(#[from] yggdrasil::YggdrasilError),
}
