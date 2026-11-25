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

/// The private game files prepared for an isolated instance.
pub fn instance_runtime(instance: &Path) -> PathBuf {
    instance.join(".axl-runtime")
}

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

    /// One directory per instance, each holding an `.axlprofile` marker.
    pub fn instances(&self) -> PathBuf {
        self.root.join("profiles")
    }

    /// Shared files used to install and launch games.
    pub fn meta(&self) -> PathBuf {
        self.root.join("meta")
    }

    /// Minecraft version folders, as the game itself lays them out.
    pub fn versions(&self) -> PathBuf {
        self.meta().join("versions")
    }

    /// One version's folder: document, client jar and game files.
    pub fn version(&self, id: &str) -> PathBuf {
        self.versions().join(id)
    }

    pub fn libraries(&self) -> PathBuf {
        self.meta().join("libraries")
    }

    pub fn assets(&self) -> PathBuf {
        self.meta().join("assets")
    }

    /// Content-addressed asset objects.
    pub fn assets_objects(&self) -> PathBuf {
        self.assets().join("objects")
    }

    /// Where one asset object lives, under the two-character prefix its hash
    /// selects.
    pub fn asset_object(&self, hash: &str) -> Option<PathBuf> {
        let prefix = hash.get(..2)?;
        Some(self.assets_objects().join(prefix).join(hash))
    }

    /// Asset index documents, as Mojang names them.
    pub fn asset_indexes(&self) -> PathBuf {
        self.assets().join("indexes")
    }

    /// The flat tree an index with `virtual` materialises, which the game reads
    /// by plain path.
    pub fn virtual_assets(&self, index_id: &str) -> PathBuf {
        self.assets().join("virtual").join(index_id)
    }

    /// The flat tree an index with `map_to_resources` materialises.
    pub fn legacy_resources(&self) -> PathBuf {
        self.meta().join("resources")
    }

    /// Logging configurations the game's JVM arguments point at.
    pub fn log_configs(&self) -> PathBuf {
        game_log_configs(&self.meta())
    }

    /// Unpacked native libraries, one folder per version.
    pub fn natives(&self) -> PathBuf {
        game_natives(&self.meta())
    }

    pub fn version_natives(&self, version_id: &str) -> PathBuf {
        self.natives().join(version_id)
    }

    /// Java runtimes this launcher manages, one folder per component.
    pub fn java_versions(&self) -> PathBuf {
        self.root.join("java")
    }

    /// Install job documents, one per job.
    pub fn jobs(&self) -> PathBuf {
        self.root.join("jobs")
    }

    /// Launcher-owned caches: fetched documents that can be rebuilt.
    pub fn caches(&self) -> PathBuf {
        self.root.join("caches")
    }

    /// The latest successful test transfer from the development panel.
    pub fn download_debug_file(&self) -> PathBuf {
        self.caches().join("download-debug.bin")
    }

    /// A unique cache directory that keeps the outputs of one development test.
    pub fn new_download_debug_batch(&self) -> io::Result<PathBuf> {
        let batches = self.caches().join("download-debug-batches");
        fs::create_dir_all(&batches)?;
        tempfile::Builder::new()
            .prefix("batch-")
            .tempdir_in(batches)
            .map(tempfile::TempDir::keep)
    }

    /// The settings document.
    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// The document holding Minecraft account identities and the selection.
    pub fn accounts(&self) -> PathBuf {
        self.root.join("accounts.json")
    }

    /// The managed Java authentication agent for third-party accounts.
    pub fn auth_agent(&self) -> PathBuf {
        self.meta().join("authlib-injector-1.2.8.jar")
    }

    /// Locks account document updates across launcher processes until the file is dropped.
    pub fn lock_accounts(&self) -> io::Result<fs::File> {
        fs::create_dir_all(&self.root)?;
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join("accounts.lock"))?;
        file.lock()?;
        Ok(file)
    }

    /// The document holding the chosen and discovered Java runtimes.
    pub fn javas(&self) -> PathBuf {
        self.root.join("javas.json")
    }
}

/// The JVM logging configuration directory under a shared game root.
pub fn game_log_configs(game_root: &Path) -> PathBuf {
    game_root.join("log_configs")
}

/// The shared directory containing one extraction directory per version.
pub fn game_natives(game_root: &Path) -> PathBuf {
    game_root.join("natives")
}

/// The content-addressed classifier archive alongside the managed game root.
pub fn native_archive(game_root: &Path, sha1: &str) -> Option<PathBuf> {
    if game_root.file_name()? != "meta"
        || sha1.len() != 40
        || !sha1.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    let layout = DataDir::new(game_root.parent()?);
    Some(
        layout
            .caches()
            .join("minecraft-natives")
            .join(format!("{sha1}.jar")),
    )
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
#[path = "../../../../tests/apps/core/storage/src/lib.rs"]
mod tests;
