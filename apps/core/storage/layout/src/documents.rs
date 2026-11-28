//! Managed document locations and the writer locks that protect them.

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::file_lock::{self, LockError, Mode};

/// A launcher document with a separately locked read-modify-write operation.
#[derive(Clone, Copy)]
pub enum Document {
    Accounts,
    DeviceUser,
    Settings,
    JavaCatalog,
    Profile,
    RuntimeReceipt,
}

impl Document {
    /// The document path under its existing app or instance directory.
    pub fn path(self, directory: &Path) -> PathBuf {
        directory.join(match self {
            Self::Accounts => "accounts.json",
            Self::DeviceUser => "device-user.json",
            Self::Settings => "settings.json",
            Self::JavaCatalog => "javas.json",
            Self::Profile => ".axlprofile",
            Self::RuntimeReceipt => ".axlauncher-complete",
        })
    }

    /// Serializes writers. Runtime installers use a nonblocking lock.
    pub fn lock(self, directory: &Path) -> Result<File, LockError> {
        let path = directory.join(match self {
            Self::Accounts => "accounts.lock",
            Self::DeviceUser => "device-user.lock",
            Self::Settings => "settings.lock",
            Self::JavaCatalog => "javas.lock",
            Self::Profile => ".axlprofile.lock",
            Self::RuntimeReceipt => ".axlauncher-install.lock",
        });
        match self {
            Self::Profile => file_lock::in_existing_directory(&path, Mode::WaitExclusive),
            Self::RuntimeReceipt => file_lock::with_parents(&path, Mode::TryExclusive),
            _ => file_lock::with_parents(&path, Mode::WaitExclusive),
        }
    }
}

/// A fixed launcher location relative to the directory owned by a workflow.
#[derive(Clone, Copy)]
pub enum ManagedFile<'name> {
    SettingsQuarantine,
    InstanceFileIndex,
    InstanceOptions,
    InstanceMods,
    JobDocument(&'name str),
    DownloadBatchOutput(usize),
}

impl ManagedFile<'_> {
    pub fn path(self, directory: &Path) -> PathBuf {
        match self {
            Self::SettingsQuarantine => directory.join("settings.json.invalid"),
            Self::InstanceFileIndex => directory.join("files.json"),
            Self::InstanceOptions => directory.join("options.txt"),
            Self::InstanceMods => directory.join("mods"),
            Self::JobDocument(id) => directory.join(format!("{id}.json")),
            Self::DownloadBatchOutput(line) => directory.join(format!("{line}.bin")),
        }
    }
}
