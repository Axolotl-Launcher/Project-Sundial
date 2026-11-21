// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Filesystem and cache primitives.

use superlogical_spi::{Storage, StorageError};

/// Stores blobs under a caller-supplied root.
#[derive(Debug)]
pub struct RootedStorage {
    root: std::path::PathBuf,
}

impl RootedStorage {
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
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
