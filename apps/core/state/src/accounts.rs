// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Minecraft account identity and a versioned account document.

use std::{fs, io};

use axlauncher_storage::{DataDir, write_atomic};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod microsoft;
mod sessions;
mod sign_in;
pub use sign_in::SignInCommit;
pub mod yggdrasil;
pub use sessions::{AccountService, AccountSnapshot, LaunchIdentity, Persistence};

// Older writers must leave the durable credential cleanup queue intact.
const SCHEMA_VERSION: u32 = 3;

/// A Minecraft profile identified within its account type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "uuid", rename_all = "snake_case")]
pub enum AccountId {
    Offline(Uuid),
    Microsoft(Uuid),
    Yggdrasil { profile: Uuid, server: Uuid },
}

impl AccountId {
    pub fn uuid(self) -> Uuid {
        match self {
            Self::Offline(uuid) | Self::Microsoft(uuid) | Self::Yggdrasil { profile: uuid, .. } => {
                uuid
            }
        }
    }

    pub fn is_online(self) -> bool {
        !matches!(self, Self::Offline(_))
    }
}

/// A saved Minecraft identity with its local credential reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    id: AccountId,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credential: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    server: Option<yggdrasil::Server>,
}

impl Account {
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
        })
    }

    pub fn id(&self) -> AccountId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn server(&self) -> Option<&yggdrasil::Server> {
        self.server.as_ref()
    }
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
    #[error("The account file uses unsupported schema {0}.")]
    UnsupportedSchema(u32),
    #[error("Could not read the account file: {0}")]
    Read(#[source] io::Error),
    #[error("The account file is unreadable: {0}")]
    Decode(#[source] serde_json::Error),
    #[error("The account file contains an invalid third-party server identity.")]
    InvalidServer,
    #[error("Could not save the account file: {0}")]
    Write(#[source] io::Error),
    #[error("Could not access the system credential store. Unlock it or sign in again.")]
    CredentialStore,
    #[error(transparent)]
    Microsoft(#[from] microsoft::MicrosoftError),
    #[error(transparent)]
    Yggdrasil(#[from] yggdrasil::YggdrasilError),
}

#[derive(Deserialize)]
struct Schema {
    schema_version: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Document {
    accounts: Vec<Account>,
    selected: Option<AccountId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    retired_credentials: Vec<Uuid>,
}

impl Document {
    fn load(data: &DataDir) -> Result<Self, AccountError> {
        match fs::read(data.accounts()) {
            Ok(bytes) => decode(&bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(AccountError::Read(error)),
        }
    }

    fn find(&self, id: AccountId) -> Result<&Account, AccountError> {
        self.accounts
            .iter()
            .find(|account| account.id == id)
            .ok_or(AccountError::Missing)
    }

    fn add_offline(&mut self, account: Account) -> Result<(), AccountError> {
        if self.find(account.id).is_ok() {
            return Err(AccountError::Duplicate);
        }
        self.selected = Some(account.id);
        self.accounts.push(account);
        Ok(())
    }

    fn select(&mut self, id: Option<AccountId>) -> Result<(), AccountError> {
        if let Some(id) = id {
            self.find(id)?;
        }
        self.selected = id;
        Ok(())
    }

    fn remove(&mut self, id: AccountId) -> Result<Option<Uuid>, AccountError> {
        let credential = self.find(id)?.credential;
        self.accounts.retain(|account| account.id != id);
        self.selected = self.selected.filter(|selected| *selected != id);
        self.retired_credentials.extend(credential);
        Ok(credential)
    }

    fn remember(&mut self, account: Account) {
        self.accounts.retain(|existing| existing.id != account.id);
        self.selected = Some(account.id);
        self.accounts.push(account);
    }

    fn update<T>(
        data: &DataDir,
        change: impl FnOnce(&mut Self) -> Result<T, AccountError>,
    ) -> Result<T, AccountError> {
        let _lock = data.lock_accounts().map_err(AccountError::Write)?;
        let mut document = Self::load(data)?;
        let output = change(&mut document)?;
        document.save(data)?;
        Ok(output)
    }

    fn save(&self, data: &DataDir) -> Result<(), AccountError> {
        let bytes = serde_json::to_vec_pretty(&Encoded {
            schema_version: SCHEMA_VERSION,
            document: self,
        })
        .expect("account document serializes");
        write_atomic(&data.accounts(), &bytes).map_err(AccountError::Write)
    }
}

#[derive(Serialize)]
struct Encoded<'a> {
    schema_version: u32,
    #[serde(flatten)]
    document: &'a Document,
}

fn decode(bytes: &[u8]) -> Result<Document, AccountError> {
    let schema: Schema = serde_json::from_slice(bytes).map_err(AccountError::Decode)?;
    if !(1..=SCHEMA_VERSION).contains(&schema.schema_version) {
        return Err(AccountError::UnsupportedSchema(schema.schema_version));
    }
    let document: Document = serde_json::from_slice(bytes).map_err(AccountError::Decode)?;
    if document
        .accounts
        .iter()
        .any(|account| !account_has_expected_server(account))
    {
        return Err(AccountError::InvalidServer);
    }
    Ok(document)
}

fn account_has_expected_server(account: &Account) -> bool {
    match account.id {
        AccountId::Yggdrasil { .. } => account
            .server()
            .is_some_and(|server| server.account_id(account.id.uuid()) == account.id),
        _ => account.server().is_none(),
    }
}
