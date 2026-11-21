// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Filesystem and cache primitives.

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
