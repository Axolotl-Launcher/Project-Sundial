// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The native jars a version needs and the files they unpack on disk.
//!
//! Two document shapes exist. A version from 1.20.1 onward lists natives as
//! ordinary libraries whose name carries a `:natives-<classifier>` suffix,
//! gated by rules. Older versions instead map an operating system to a
//! classifier key through `natives`, with the arch filled into `${arch}`.
//! Selection is pure; extraction reads through [`NativeReader`], so the
//! unpacking logic does not depend on a particular zip decoder.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use axlauncher_storage::GameDir;

use crate::rules::{self, Target};
use crate::version::{Artifact, Library};

/// The placeholder older documents use inside a classifier key.
const ARCH_PLACEHOLDER: &str = "${arch}";

/// The game's classifier spelling for a 64-bit x86 target.
const GAME_ARCH_64: &str = "64";

/// The game's classifier spelling for a 32-bit x86 target.
const GAME_ARCH_32: &str = "32";

/// The target vocabulary's 64-bit x86 name.
const TARGET_ARCH_X86_64: &str = "x86_64";

/// The target vocabulary's 32-bit x86 name.
const TARGET_ARCH_X86: &str = "x86";

/// Extensions a native library can carry.
const NATIVE_EXTENSIONS: [&str; 4] = ["dll", "so", "dylib", "jnilib"];

/// How many ignored entry names are worth reporting to a caller.
const MAX_SKIPPED_NAMES: usize = 64;

/// Why a natives directory could not be produced.
#[derive(Debug)]
pub enum NativeError {
    /// A file could not be read or written.
    Io {
        /// The file the operation was aimed at.
        path: PathBuf,
        /// The underlying filesystem failure.
        source: io::Error,
    },
    /// The jar is a zip archive but its contents could not be decoded.
    Zip {
        /// The jar being read.
        path: PathBuf,
        /// What went wrong while decoding it.
        reason: String,
    },
    /// The file is not a zip archive, so it cannot hold natives.
    NotAJar {
        /// The file that was expected to be a jar.
        path: PathBuf,
    },
}

impl std::fmt::Display for NativeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "cannot access {}: {source}", path.display())
            }
            Self::Zip { path, reason } => {
                write!(formatter, "cannot read jar {}: {reason}", path.display())
            }
            Self::NotAJar { path } => {
                write!(formatter, "{} is not a jar", path.display())
            }
        }
    }
}

impl std::error::Error for NativeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Zip { .. } | Self::NotAJar { .. } => None,
        }
    }
}

/// A jar that carries native libraries for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeEntry {
    /// The jar's path relative to the libraries root, as the document names it.
    pub path: String,
}

/// What an extraction produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    /// The natives written under the destination, each path listed once.
    pub files: Vec<PathBuf>,
    /// Non-native entry names that were ignored, sorted and capped.
    pub skipped: Vec<String>,
}

/// A source of jar entries, so extraction can be tested without a zip decoder.
///
/// An implementation that opens a file and finds it is not a zip archive
/// reports [`NativeError::NotAJar`] with that file's path.
pub trait NativeReader {
    /// The entries of the jar at `path`, as `(name, bytes)` pairs.
    fn read_jar(&self, path: &Path) -> Result<Vec<(String, Vec<u8>)>, NativeError>;
}

/// The native jars that apply on `target`, in document order.
pub fn entries(libraries: &[Library], target: &Target) -> Vec<NativeEntry> {
    libraries
        .iter()
        .filter_map(|library| {
            classpath_native(library, target).or_else(|| classifier_download(library, target))
        })
        .map(|artifact| NativeEntry {
            path: artifact.path.clone(),
        })
        .collect()
}

/// Extracts the native files of `entries` into `destination`.
///
/// The destination is created when missing, and an existing native is replaced
/// rather than treated as an error. Each file is written atomically, so a
/// failed extraction never leaves a truncated native behind.
pub fn extract<R: NativeReader>(
    reader: &R,
    jars_root: &Path,
    entries: &[NativeEntry],
    destination: &Path,
) -> Result<Extracted, NativeError> {
    extract_archives(
        reader,
        entries.iter().map(|entry| jars_root.join(&entry.path)),
        destination,
    )
}

/// Extracts the archives selected for a managed version under a version lock.
pub(crate) fn extract_version(
    reader: &impl NativeReader,
    game_root: &GameDir,
    version_id: &str,
    libraries: &[Library],
    target: &Target,
) -> Result<Extracted, NativeError> {
    let lock_path = game_root.natives_lock(version_id);
    let library_root = game_root.libraries();
    let lock = (|| {
        fs::create_dir_all(
            lock_path
                .parent()
                .expect("native lock is inside the game tree"),
        )?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)?;
        file.lock()?;
        Ok::<_, io::Error>(file)
    })()
    .map_err(|source| NativeError::Io {
        path: lock_path,
        source,
    })?;

    let archives = libraries.iter().filter_map(|library| {
        if let Some(artifact) = classifier_download(library, target) {
            return Some(
                game_root
                    .native_classifier_archive(Path::new(&artifact.path), artifact.sha1.as_deref()),
            );
        }
        classpath_native(library, target).map(|artifact| library_root.join(&artifact.path))
    });
    let extracted = extract_archives(reader, archives, &game_root.version_natives(version_id));
    drop(lock);
    extracted
}

fn extract_archives(
    reader: &impl NativeReader,
    archives: impl IntoIterator<Item = PathBuf>,
    destination: &Path,
) -> Result<Extracted, NativeError> {
    fs::create_dir_all(destination).map_err(|source| NativeError::Io {
        path: destination.to_path_buf(),
        source,
    })?;

    let mut files = Vec::new();
    let mut skipped = BTreeSet::new();
    for jar in archives {
        for (name, bytes) in reader.read_jar(&jar)? {
            match native_relative_path(&name) {
                Some(relative) => write_native(destination, &relative, &bytes, &mut files)?,
                None => record_skipped(&mut skipped, name),
            }
        }
    }

    Ok(Extracted {
        files,
        skipped: skipped.into_iter().collect(),
    })
}

/// The artifact of a native coordinate that goes on the classpath.
fn classpath_native<'a>(library: &'a Library, target: &Target) -> Option<&'a Artifact> {
    if !library.is_classified_native() || !rules::library_applies(library, target) {
        return None;
    }
    library.downloads.as_ref()?.artifact.as_ref()
}

/// The artifact selected from the older platform-to-classifier map.
pub fn classifier_download<'a>(library: &'a Library, target: &Target) -> Option<&'a Artifact> {
    if library.is_classified_native() || !rules::library_applies(library, target) {
        return None;
    }
    let classifier = library.natives.as_ref()?.get(target.os.as_str())?;
    let classifier = classifier.replace(ARCH_PLACEHOLDER, classifier_arch(&target.arch));
    library
        .downloads
        .as_ref()
        .and_then(|downloads| downloads.classifiers.as_ref())
        .and_then(|classifiers| classifiers.get(&classifier))
}

/// The classifier spelling of `arch`, which the game writes as `64` or `32`.
fn classifier_arch(arch: &str) -> &str {
    match arch {
        TARGET_ARCH_X86_64 => GAME_ARCH_64,
        TARGET_ARCH_X86 => GAME_ARCH_32,
        other => other,
    }
}

/// The safe relative path of an entry when it is a native file.
fn native_relative_path(entry: &str) -> Option<PathBuf> {
    if entry.ends_with('/') || entry.ends_with('\\') {
        return None;
    }
    let extension = Path::new(entry).extension()?.to_str()?.to_ascii_lowercase();
    if !NATIVE_EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    safe_components(Path::new(entry))
}

/// The path with only normal components kept, rejecting anything that escapes.
fn safe_components(path: &Path) -> Option<PathBuf> {
    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if safe.as_os_str().is_empty() {
        None
    } else {
        Some(safe)
    }
}

/// Writes one native and records its path.
fn write_native(
    destination: &Path,
    relative: &Path,
    bytes: &[u8],
    files: &mut Vec<PathBuf>,
) -> Result<(), NativeError> {
    let target = destination.join(relative);
    if !fs::read(&target).is_ok_and(|existing| existing == bytes) {
        axlauncher_storage::write_atomic(&target, bytes).map_err(|source| NativeError::Io {
            path: target.clone(),
            source,
        })?;
    }
    if !files.contains(&target) {
        files.push(target);
    }
    Ok(())
}

/// Keeps the ignored name for logging while the list still has room.
fn record_skipped(skipped: &mut BTreeSet<String>, name: String) {
    if skipped.len() < MAX_SKIPPED_NAMES {
        skipped.insert(name);
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/natives/tests.rs"]
mod tests;
