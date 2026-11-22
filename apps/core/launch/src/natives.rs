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

use crate::rules::{self, Target};
use crate::version::Library;

/// The marker that distinguishes a native library from a classpath library.
const NATIVES_MARKER: &str = ":natives-";

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
        .filter(|library| rules::library_applies(library, target))
        .filter_map(|library| native_jar(library, target))
        .map(|path| NativeEntry { path })
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
    fs::create_dir_all(destination).map_err(|source| NativeError::Io {
        path: destination.to_path_buf(),
        source,
    })?;

    let mut files = Vec::new();
    let mut skipped = BTreeSet::new();
    for entry in entries {
        let jar = jars_root.join(&entry.path);
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

/// The jar of one library that holds natives, when it names one for `target`.
fn native_jar(library: &Library, target: &Target) -> Option<String> {
    if library.name.contains(NATIVES_MARKER) {
        return library
            .downloads
            .as_ref()
            .and_then(|downloads| downloads.artifact.as_ref())
            .map(|artifact| artifact.path.clone());
    }

    let classifier = library.natives.as_ref()?.get(target.os.as_str())?;
    let classifier = classifier.replace(ARCH_PLACEHOLDER, classifier_arch(&target.arch));
    library
        .downloads
        .as_ref()
        .and_then(|downloads| downloads.classifiers.as_ref())
        .and_then(|classifiers| classifiers.get(&classifier))
        .map(|artifact| artifact.path.clone())
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
    axlauncher_storage::write_atomic(&target, bytes).map_err(|source| NativeError::Io {
        path: target.clone(),
        source,
    })?;
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
mod tests;
