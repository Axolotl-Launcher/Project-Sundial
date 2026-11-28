// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs::{self, File};
use std::io::{self, Seek};
use std::path::Path;

/// A private file written alongside its destination and removed on drop.
pub struct AtomicFile(tempfile::NamedTempFile);

impl AtomicFile {
    /// The staging file shares the destination's filesystem for atomic promotion.
    pub fn new(destination: &Path) -> io::Result<Self> {
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty());
        let parent = parent.unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        tempfile::NamedTempFile::new_in(parent).map(Self)
    }

    /// The staged file, supporting both sequential and random-access writes.
    pub fn file(&mut self) -> &mut File {
        self.0.as_file_mut()
    }

    /// The staged file positioned at its first byte for verification.
    pub fn rewind(&mut self) -> io::Result<&mut File> {
        let file = self.file();
        file.rewind()?;
        Ok(file)
    }

    /// Atomically replaces the destination with the completed file.
    pub fn commit(self, destination: &Path) -> io::Result<()> {
        self.0
            .persist(destination)
            .map(|_| ())
            .map_err(|error| error.error)
    }

    pub(super) fn commit_new(self, destination: &Path) -> io::Result<()> {
        self.0
            .persist_noclobber(destination)
            .map(|_| ())
            .map_err(|error| error.error)
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/storage/src/atomic_file.rs"]
mod tests;
