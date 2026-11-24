// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The game files a version document names that the game root does not hold.
//!
//! Planning is separate from downloading: the planners only read the
//! filesystem and return what is absent or the wrong size, so they run without
//! a network and are tested against a fixture directory.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub use axlauncher_download_engine::{
    DEFAULT_CONCURRENCY, DownloadOutcome, MissingFile, download_all,
};
use axlauncher_launch::natives::classifier_download;
use axlauncher_launch::rules::{self, Target};
use axlauncher_launch::version::{Artifact, Library};
use axlauncher_storage::write_atomic;
use serde::Deserialize;

/// The directory under the game root that holds library artifacts.
const LIBRARIES_DIR: &str = "libraries";

/// The directory under the game root that holds version folders.
const VERSIONS_DIR: &str = "versions";

/// The directory under the game root that holds the asset store.
const ASSETS_DIR: &str = "assets";

/// The directory under `assets` that holds content-addressed objects.
const OBJECTS_DIR: &str = "objects";

/// How many leading hash characters select an asset object's directory.
const OBJECT_PREFIX_LEN: usize = 2;

/// Where Mojang serves asset objects, as the asset index format defines them.
const ASSET_OBJECT_BASE: &str = "https://resources.download.minecraft.net";

/// The asset index document, reduced to what an install reads.
///
/// Most indexes only list hashed objects. Two older shapes additionally ask for
/// a flat tree the game reads by plain path: `virtual` (1.7–1.8) wants it under
/// `assets/virtual/<index id>`, and `map_to_resources` (1.5 and older) wants it
/// under `resources/`. Both flags are part of the document, so the decision is
/// read rather than guessed from a version number.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AssetIndexDocument {
    pub objects: BTreeMap<String, AssetObject>,
    #[serde(default, rename = "virtual")]
    pub virtual_map: bool,
    #[serde(default)]
    pub map_to_resources: bool,
}

impl AssetIndexDocument {
    /// Where the flat tree belongs, when the index asks for one.
    ///
    /// Read from the raw key names, because both are spelled with underscores
    /// in the document and would otherwise need aliases on every field.
    pub fn flat_root(&self, game_root: &Path, index_id: &str) -> Option<PathBuf> {
        if self.virtual_map {
            return Some(game_root.join(ASSETS_DIR).join("virtual").join(index_id));
        }
        if self.map_to_resources {
            return Some(game_root.join("resources"));
        }
        None
    }
}

/// One object in an asset index.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

impl AssetIndexDocument {
    /// The document encoded in `bytes`, as Mojang publishes it.
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

/// The library artifacts `libraries` names for `target` that are absent or the
/// wrong size under `game_root`.
///
/// The list includes jars that are never classpath entries, such as the native
/// classifier archives a modern manifest declares: the game still needs them
/// extracted, so an install that skipped them would leave `natives/` empty.
pub fn missing_libraries(
    game_root: &Path,
    libraries: &[Library],
    target: &Target,
) -> Vec<MissingFile> {
    missing_library_files(game_root, libraries, target, false)
}

pub(crate) fn missing_library_files(
    game_root: &Path,
    libraries: &[Library],
    target: &Target,
    include_local: bool,
) -> Vec<MissingFile> {
    let root = game_root.join(LIBRARIES_DIR);
    let mut destinations = std::collections::HashSet::new();
    libraries
        .iter()
        .filter(|library| rules::library_applies(library, target))
        .flat_map(|library| {
            let regular = library
                .download()
                .and_then(|artifact| contained_artifact(&root, artifact));
            let classifier = classifier_download(library, target).and_then(|artifact| {
                let (maven_path, artifact) = contained_artifact(&root, artifact.clone())?;
                let path = artifact
                    .sha1
                    .as_deref()
                    .and_then(|sha1| axlauncher_storage::native_archive(game_root, sha1))
                    .filter(|_| needs_download(&maven_path, artifact.size))
                    .unwrap_or(maven_path);
                Some((path, artifact))
            });
            regular.into_iter().chain(classifier)
        })
        .filter_map(|(path, artifact)| {
            // An artifact whose stated URL is empty is produced on this machine,
            // not downloaded: a loader's patched client jar is written there by
            // its processors. Planning a fetch for it would fail every install.
            if !include_local && artifact.url.as_deref() == Some("") {
                return None;
            }
            (missing_artifact(&path, &artifact) && destinations.insert(path.clone())).then(|| {
                MissingFile {
                    // An artifact without a URL stays in the plan, so the download
                    // reports it instead of the plan hiding it.
                    url: artifact.url.clone().unwrap_or_default(),
                    path,
                    sha1: artifact.sha1.clone(),
                    sha1_alternatives: artifact.sha1_alternatives,
                    size: artifact.size,
                }
            })
        })
        .collect()
}

fn missing_artifact(path: &Path, artifact: &Artifact) -> bool {
    if needs_download(path, artifact.size) {
        return true;
    }
    if artifact.sha1_alternatives.is_empty() {
        return false;
    }
    !axlauncher_storage::open_for_read(path)
        .and_then(|file| {
            axlauncher_network::verify_reader_any(
                file,
                artifact.sha1.as_deref(),
                &artifact.sha1_alternatives,
            )
        })
        .unwrap_or(false)
}

/// The artifact's destination under `root`, or `None` when its stated path
/// would leave it.
///
/// A library path comes from a network answer — a loader document can write
/// anywhere it likes — so a path with a parent component or a root is dropped
/// rather than joined into the game tree.
fn contained_artifact(root: &Path, artifact: Artifact) -> Option<(PathBuf, Artifact)> {
    let path = Path::new(&artifact.path);
    if path
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return Some((root.join(path), artifact));
    }
    None
}

/// The client jar a version downloads, when its document names one.
///
/// A loader profile usually names the vanilla jar it runs from; writing it
/// under that jar's own folder is what makes the profile launchable without the
/// vanilla version having been installed separately.
pub fn client_jar(
    game_root: &Path,
    version: &axlauncher_launch::version::Version,
) -> Option<MissingFile> {
    let download = version.downloads.as_ref()?.client.as_ref()?;
    let path = game_root
        .join(VERSIONS_DIR)
        .join(version.jar_id())
        .join(format!("{}.jar", version.jar_id()));
    needs_download(&path, download.size).then(|| MissingFile {
        url: download.url.clone(),
        path,
        sha1: download.sha1.clone(),
        sha1_alternatives: Vec::new(),
        size: download.size,
    })
}

/// The logging configuration the game's JVM arguments point at.
///
/// The document names it by id under `log_configs`, which is where the
/// argument this build assembles looks for it.
pub fn log_config(
    game_root: &Path,
    version: &axlauncher_launch::version::Version,
) -> Option<MissingFile> {
    let client = version.logging.as_ref()?.client.as_ref()?;
    let path = axlauncher_storage::game_log_configs(game_root).join(&client.file.id);
    needs_download(&path, client.file.size).then(|| MissingFile {
        url: client.file.url.clone(),
        path,
        sha1: client.file.sha1.clone(),
        sha1_alternatives: Vec::new(),
        size: client.file.size,
    })
}

/// The asset objects `index` names that are absent or the wrong size under
/// `game_root`. Multiple names for the same object share a single download.
pub fn missing_assets(game_root: &Path, index: &AssetIndexDocument) -> Vec<MissingFile> {
    let root = game_root.join(ASSETS_DIR).join(OBJECTS_DIR);
    let mut hashes = HashSet::new();
    index
        .objects
        .values()
        .filter_map(|object| {
            let prefix = object.hash.get(..OBJECT_PREFIX_LEN)?;
            let hash = &object.hash;
            if !hashes.insert(hash) {
                return None;
            }
            let path = root.join(prefix).join(hash);
            needs_download(&path, Some(object.size)).then(|| MissingFile {
                url: format!("{ASSET_OBJECT_BASE}/{prefix}/{hash}"),
                path,
                sha1: Some(hash.clone()),
                sha1_alternatives: Vec::new(),
                size: Some(object.size),
            })
        })
        .collect()
}

/// Whether `path` is absent or has a size other than `expected_size`.
///
/// A present file with no expected size counts as complete: there is nothing to
/// compare against.
fn needs_download(path: &Path, expected_size: Option<u64>) -> bool {
    let existing = fs::metadata(path)
        .ok()
        .filter(fs::Metadata::is_file)
        .map(|metadata| metadata.len());
    match (existing, expected_size) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(size), Some(expected)) => size != expected,
    }
}

/// How many entries of a flat asset tree a report names individually.
const MAX_FLAT_FAILURES: usize = 8;

/// Copies every object `index` lists into the flat tree the game reads.
///
/// This runs after the objects are on disk, because the objects are its only
/// source: the game wants `sounds/step/grass1.ogg`, and the store holds it
/// under its hash. A file already in place with the right size is left alone,
/// so a re-run costs one metadata call per asset.
///
/// Failure paths are capped at [`MAX_FLAT_FAILURES`] so a wholly unwritable
/// tree reports the reason rather than one line per asset.
pub fn materialise_flat(
    game_root: &Path,
    index_id: &str,
    index: &AssetIndexDocument,
) -> Vec<String> {
    index
        .flat_root(game_root, index_id)
        .map_or_else(Vec::new, |root| {
            materialise_flat_at(game_root, &root, index)
        })
}

fn materialise_flat_at(game_root: &Path, root: &Path, index: &AssetIndexDocument) -> Vec<String> {
    let objects = game_root.join(ASSETS_DIR).join(OBJECTS_DIR);
    let mut failures = Vec::new();

    for (name, object) in &index.objects {
        let Some(prefix) = object.hash.get(..OBJECT_PREFIX_LEN) else {
            continue;
        };
        let source = objects.join(prefix).join(&object.hash);
        let destination = root.join(name.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !needs_download(&destination, Some(object.size)) {
            continue;
        }
        let Err(error) = copy_object(&source, &destination) else {
            continue;
        };
        if failures.len() < MAX_FLAT_FAILURES {
            failures.push(format!("{}: {error}", destination.display()));
        }
    }
    failures
}

/// The first missing or incomplete copy in the tree the game reads.
pub(crate) fn missing_flat_asset(
    game_root: &Path,
    index_id: &str,
    index: &AssetIndexDocument,
) -> Option<PathBuf> {
    let root = index.flat_root(game_root, index_id)?;
    index.objects.iter().find_map(|(name, object)| {
        let path = root.join(name.replace('/', std::path::MAIN_SEPARATOR_STR));
        needs_download(&path, Some(object.size)).then_some(path)
    })
}

/// Copies one object into the flat tree, creating its parent.
fn copy_object(source: &Path, destination: &Path) -> std::io::Result<()> {
    let bytes = fs::read(source)?;
    write_atomic(destination, &bytes)
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/install/src/assets/tests.rs"]
mod tests;
