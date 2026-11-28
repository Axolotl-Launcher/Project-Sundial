//! File locks used to coordinate access to launcher-managed data.

use std::fmt;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::instance_paths::PROFILE_LOCK_PREFIX;

#[derive(Debug)]
pub struct LockError {
    path: PathBuf,
    source: io::Error,
}

impl LockError {
    pub fn into_parts(self) -> (PathBuf, io::Error) {
        (self.path, self.source)
    }
}

impl fmt::Display for LockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.source)
    }
}

impl std::error::Error for LockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

impl From<LockError> for io::Error {
    fn from(error: LockError) -> Self {
        error.source
    }
}

pub(super) enum Mode {
    WaitExclusive,
    TryExclusive,
    TryShared,
}

pub(super) fn in_existing_directory(path: &Path, mode: Mode) -> Result<File, LockError> {
    acquire(path, mode).map_err(|source| LockError {
        path: path.to_owned(),
        source,
    })
}

pub(super) fn profile(directory: &Path) -> Result<File, LockError> {
    let directory_error = |source| LockError {
        path: directory.to_owned(),
        source,
    };
    if !directory
        .symlink_metadata()
        .map_err(directory_error)?
        .is_dir()
    {
        return Err(directory_error(io::ErrorKind::NotADirectory.into()));
    }
    let canonical = fs::canonicalize(directory).map_err(directory_error)?;
    let name = canonical
        .file_name()
        .ok_or_else(|| directory_error(io::ErrorKind::InvalidInput.into()))?;
    // A fixed-size sibling filename fits even when the instance name does not
    // have room for a suffix. No directory in the instance list owns the locks.
    let digest = hex::encode(Sha256::digest(name.as_encoded_bytes()));
    let lock_path = canonical.with_file_name(format!("{PROFILE_LOCK_PREFIX}{digest}"));
    in_existing_directory(&lock_path, Mode::WaitExclusive)
}

pub(super) fn with_parents(path: &Path, mode: Mode) -> Result<File, LockError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| LockError {
            path: parent.to_owned(),
            source,
        })?;
    }
    in_existing_directory(path, mode)
}

fn acquire(path: &Path, mode: Mode) -> io::Result<File> {
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    match mode {
        Mode::WaitExclusive => file.lock()?,
        Mode::TryExclusive => file.try_lock()?,
        Mode::TryShared => file.try_lock_shared()?,
    }
    Ok(file)
}
