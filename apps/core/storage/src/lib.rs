// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Filesystem and cache primitives.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use superlogical_spi::{Storage, StorageError};

/// The launcher's on-disk root and the directories under it.
///
/// One type owns the layout so a path is never assembled from string literals
/// at a call site. Adding a directory means adding a method here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// One directory per instance, each holding an `instance.json`.
    pub fn instances(&self) -> PathBuf {
        self.root.join("instances")
    }

    /// Minecraft version folders, as the game itself lays them out.
    pub fn versions(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }

    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }

    /// The settings document.
    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }
}

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
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "superlogical-storage-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("temp directory");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn write_atomic_replaces_the_target_and_leaves_no_temporary() {
        let directory = TempDir::new();
        let target = directory.0.join("settings.json");
        fs::write(&target, b"old").expect("seed");

        write_atomic(&target, b"new").expect("write");

        assert_eq!(fs::read(&target).expect("read"), b"new");
        assert!(!directory.0.join("settings.json.tmp").exists());
    }

    #[test]
    fn write_atomic_creates_missing_parents() {
        let directory = TempDir::new();
        let target = directory.0.join("nested").join("file.json");

        write_atomic(&target, b"data").expect("write");

        assert_eq!(fs::read(&target).expect("read"), b"data");
    }
}
