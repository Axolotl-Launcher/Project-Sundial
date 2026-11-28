//! Game tree paths and locks shared by install and launch.

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::file_lock::{self, LockError, Mode};

const SHA1_HEX_LENGTH: usize = 40;

/// The access to protect with a launcher-managed game lock.
#[derive(Clone, Copy)]
pub enum GameAccess<'id> {
    Install,
    Launch,
    ExtractNatives(&'id str),
}

/// A game version's document, jar, extracted natives or logging configuration.
#[derive(Clone, Copy)]
pub enum VersionPath {
    Document,
    ClientJar,
    Natives,
    LogConfig,
    ProcessorIncomplete,
}

/// Positions named by an asset index.
#[derive(Clone, Copy)]
pub enum AssetPath<'id> {
    Index(&'id str),
    Virtual(&'id str),
}

/// An installed game tree, with a native archive cache only for shared installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameDir {
    root: PathBuf,
    versions: PathBuf,
    libraries: PathBuf,
    assets: PathBuf,
    legacy_resources: PathBuf,
    native_cache: Option<PathBuf>,
}

impl GameDir {
    pub(super) fn shared(root: PathBuf, native_cache: PathBuf) -> Self {
        Self::new(root, Some(native_cache))
    }

    fn new(root: PathBuf, native_cache: Option<PathBuf>) -> Self {
        Self {
            versions: root.join("versions"),
            libraries: root.join("libraries"),
            assets: root.join("assets"),
            legacy_resources: root.join("resources"),
            root,
            native_cache,
        }
    }

    /// A caller-supplied game tree that must not use the shared archive cache.
    pub fn standalone(root: impl Into<PathBuf>) -> Self {
        Self::new(root.into(), None)
    }

    /// The private game files prepared for a portable instance.
    pub fn isolated(instance: &Path) -> Self {
        Self::standalone(instance.join(".axl-runtime"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn versions(&self) -> &Path {
        &self.versions
    }

    pub fn version_path(&self, id: &str, path: VersionPath) -> PathBuf {
        match path {
            VersionPath::Document => self.versions.join(id).join(format!("{id}.json")),
            VersionPath::ClientJar => self.versions.join(id).join(format!("{id}.jar")),
            VersionPath::Natives => self.root.join("natives").join(id),
            VersionPath::LogConfig => self.root.join("log_configs").join(id),
            VersionPath::ProcessorIncomplete => {
                self.versions.join(id).join(".processors-incomplete")
            }
        }
    }

    pub fn libraries(&self) -> &Path {
        &self.libraries
    }

    pub fn assets(&self) -> &Path {
        &self.assets
    }

    pub fn asset_object(&self, hash: &str) -> Option<PathBuf> {
        let prefix = hash.get(..2)?;
        if !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        Some(self.assets().join("objects").join(prefix).join(hash))
    }

    pub fn asset_path(&self, path: AssetPath<'_>) -> PathBuf {
        match path {
            AssetPath::Index(id) => self.assets.join("indexes").join(format!("{id}.json")),
            AssetPath::Virtual(id) => self.assets.join("virtual").join(id),
        }
    }

    pub fn legacy_resources(&self) -> &Path {
        &self.legacy_resources
    }

    /// Install and launch try the whole tree; extraction waits for its version.
    pub fn lock(&self, access: GameAccess<'_>) -> Result<File, LockError> {
        let (path, mode) = match access {
            GameAccess::Install => (self.root.join(".install.lock"), Mode::TryExclusive),
            GameAccess::Launch => (self.root.join(".install.lock"), Mode::TryShared),
            GameAccess::ExtractNatives(id) => (
                self.root
                    .join("natives")
                    .join(".locks")
                    .join(format!("{id}.lock")),
                Mode::WaitExclusive,
            ),
        };
        file_lock::with_parents(&path, mode)
    }

    /// The single location of an archive from a manifest's native-classifier map.
    ///
    /// A shared tree caches it by SHA-1 when the hash is usable. Other trees
    /// and classifiers without a usable hash use the manifest's Maven layout.
    pub fn native_classifier_archive(&self, maven_relative: &Path, sha1: Option<&str>) -> PathBuf {
        if let (Some(cache), Some(sha1)) = (&self.native_cache, sha1)
            && sha1.len() == SHA1_HEX_LENGTH
            && sha1.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return cache.join(format!("{sha1}.jar"));
        }
        self.libraries().join(maven_relative)
    }
}
