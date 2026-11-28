//! Locks the managed account file and exposes only the current device user's records.

use std::{collections::BTreeMap, fs, io, path::PathBuf};

use axlauncher_storage::{DataDir, DataPath, Document, write_atomic};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::hierarchy::DeviceRootUser;
use super::{AccountError, AccountId, CredentialRef, McAccount};

const SCHEMA_VERSION: u32 = 4;

pub(super) struct AccountFile {
    data: DataDir,
    principal: String,
}

impl AccountFile {
    pub(super) fn new(data: DataDir, principal: String) -> Self {
        Self { data, principal }
    }

    fn path(&self) -> PathBuf {
        Document::Accounts.path(self.data.path(DataPath::Root))
    }

    pub(super) fn credential_key(&self, reference: CredentialRef) -> String {
        match reference {
            CredentialRef::Legacy(uuid) => uuid.to_string(),
            CredentialRef::Scoped { key } => format!("{}:{key}", self.principal),
        }
    }

    pub(super) fn read(&self) -> Result<DeviceRootUser, AccountError> {
        Ok(self.lock()?.root)
    }

    pub(super) fn update<T>(
        &self,
        change: impl FnOnce(&mut DeviceRootUser) -> Result<T, AccountError>,
    ) -> Result<T, AccountError> {
        let mut transaction = self.lock()?;
        let output = change(transaction.root_mut())?;
        transaction.save()?;
        Ok(output)
    }

    pub(super) fn lock(&self) -> Result<Transaction<'_>, AccountError> {
        let lock = Document::Accounts
            .lock(self.data.path(DataPath::Root))
            .map_err(|error| AccountError::Write(error.into()))?;
        let (mut file, migrated) = match fs::read(self.path()) {
            Ok(bytes) => FileData::decode(&bytes, &self.principal)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => (FileData::default(), false),
            Err(error) => return Err(AccountError::Read(error)),
        };
        let mut root = file
            .device_roots
            .remove(&self.principal)
            .map(serde_json::from_value::<DeviceRootUser>)
            .transpose()
            .map_err(|_| AccountError::Decode)?
            .unwrap_or_default();
        root.validate()?;
        let initialized = root.ensure_local();
        let mut transaction = Transaction {
            owner: self,
            _lock: lock,
            file,
            root,
        };
        if migrated || initialized {
            transaction.save()?;
        }
        Ok(transaction)
    }
}

pub(super) struct Transaction<'file> {
    owner: &'file AccountFile,
    _lock: fs::File,
    file: FileData,
    root: DeviceRootUser,
}

impl Transaction<'_> {
    pub(super) fn root(&self) -> &DeviceRootUser {
        &self.root
    }

    pub(super) fn root_mut(&mut self) -> &mut DeviceRootUser {
        &mut self.root
    }

    pub(super) fn save(&mut self) -> Result<(), AccountError> {
        self.root().validate()?;
        let value = serde_json::to_value(self.root()).expect("device root serializes");
        self.file
            .device_roots
            .insert(self.owner.principal.clone(), value);
        let bytes = serde_json::to_vec_pretty(&self.file).expect("account document serializes");
        write_atomic(&self.owner.path(), &bytes).map_err(AccountError::Write)
    }
}

#[derive(Deserialize)]
struct Schema {
    schema_version: u32,
}

#[derive(Deserialize, Serialize)]
struct FileData {
    schema_version: u32,
    device_roots: BTreeMap<String, Value>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl Default for FileData {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            device_roots: BTreeMap::new(),
            extra: Map::new(),
        }
    }
}

impl FileData {
    fn decode(bytes: &[u8], principal: &str) -> Result<(Self, bool), AccountError> {
        let schema: Schema = serde_json::from_slice(bytes).map_err(|_| AccountError::Decode)?;
        match schema.schema_version {
            SCHEMA_VERSION => serde_json::from_slice(bytes)
                .map(|file| (file, false))
                .map_err(|_| AccountError::Decode),
            1..SCHEMA_VERSION => {
                let old: FlatAccounts =
                    serde_json::from_slice(bytes).map_err(|_| AccountError::Decode)?;
                Ok((old.migrate(principal), true))
            }
            unsupported => Err(AccountError::UnsupportedSchema(unsupported)),
        }
    }
}

#[derive(Deserialize)]
struct FlatAccounts {
    #[serde(rename = "schema_version")]
    _schema: u32,
    accounts: Vec<McAccount>,
    selected: Option<AccountId>,
    #[serde(default)]
    retired_credentials: Vec<CredentialRef>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl FlatAccounts {
    fn migrate(self, principal: &str) -> FileData {
        let root =
            DeviceRootUser::from_flat(self.accounts, self.selected, self.retired_credentials);
        let mut file = FileData {
            extra: self.extra,
            ..FileData::default()
        };
        if let Some(value) = file.extra.remove("device_roots") {
            file.extra.insert("legacy_device_roots".to_owned(), value);
        }
        file.device_roots.insert(
            principal.to_owned(),
            serde_json::to_value(root).expect("migrated device root serializes"),
        );
        file
    }
}
