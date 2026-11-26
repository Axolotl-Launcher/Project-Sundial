// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Filesystem and cache primitives.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_spi::{Storage, StorageError};

mod atomic_file;
pub use atomic_file::AtomicFile;
mod credentials;
pub use credentials::SystemCredentials;

/// Opens an existing file for sequential verification.
pub fn open_for_read(path: &Path) -> io::Result<fs::File> {
    fs::File::open(path)
}

mod console_log;
pub use console_log::open_instance_console_log;

mod game_dir;
pub use game_dir::{GameDir, version_document};
mod instance_paths;
pub use instance_paths::{instance_file_index, instance_options, profile_document, stage_instance};
mod paths;
pub use paths::{
    DataDir, Directory, WorkerLibrary, download_debug_batch_file, invalid_settings_path,
    javas_path, job_document, runtime_record, settings_lock_path, settings_path, user_data_dir,
    user_temp_dir,
};

/// Replaces `path` with `bytes` by writing a sibling temporary file and renaming
/// it over the target.
///
/// A half-written settings file is worse than no file: the next start would
/// read a truncated document. `fs::rename` replaces an existing target on every
/// platform this project targets, so a reader sees either the old document or
/// the new one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    fs::create_dir_all(parent)?;

    let mut temporary: OsString = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);

    fs::write(&temporary, bytes)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

/// Stores blobs under a caller-supplied root.
#[derive(Debug)]
pub struct RootedStorage {
    root: PathBuf,
}

impl RootedStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl Storage for RootedStorage {
    fn read(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        std::fs::read(self.root.join(path))
            .map_err(|error| StorageError::NotFound(error.to_string()))
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), StorageError> {
        let target = self.root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| StorageError::Denied(error.to_string()))?;
        }
        std::fs::write(&target, bytes).map_err(|error| StorageError::Denied(error.to_string()))
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/storage/src/lib.rs"]
mod tests;
