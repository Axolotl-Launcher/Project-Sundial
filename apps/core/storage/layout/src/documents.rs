//! Managed document locations and the writer locks that protect them.

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::file_lock::{self, LockError, Mode};

/// A launcher document with a separately locked read-modify-write operation.
#[derive(Clone, Copy)]
pub enum Document {
    Accounts,
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
            Self::Settings => "settings.json",
            Self::JavaCatalog => "javas.json",
            Self::Profile => ".axlprofile",
            Self::RuntimeReceipt => ".axlauncher-complete",
        })
    }

    /// Serializes writers. Runtime installers use a nonblocking lock.
    pub fn lock(self, directory: &Path) -> Result<File, LockError> {
        let (name, mode) = match self {
            Self::Profile => {
                // Windows cannot move a directory while its lock file is open inside it.
                return file_lock::profile(directory);
            }
            Self::Accounts => ("accounts.lock", Mode::WaitExclusive),
            Self::Settings => ("settings.lock", Mode::WaitExclusive),
            Self::JavaCatalog => ("javas.lock", Mode::WaitExclusive),
            Self::RuntimeReceipt => (".axlauncher-install.lock", Mode::TryExclusive),
        };
        file_lock::with_parents(&directory.join(name), mode)
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
