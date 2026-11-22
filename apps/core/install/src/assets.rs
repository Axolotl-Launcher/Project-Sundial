// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The game files a version document names that the game root does not hold.
//!
//! Planning is separate from downloading: the planners only read the
//! filesystem and return what is absent or the wrong size, so they run without
//! a network and are tested against a fixture directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use axlauncher_launch::natives::classifier_download;
use axlauncher_launch::rules::{self, Target};
use axlauncher_launch::version::Library;
use axlauncher_network::verify;
use axlauncher_spi::{FetchError, Network};
use axlauncher_storage::write_atomic;
use futures::stream::{self, StreamExt};
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

/// The number of downloads in flight when a caller passes zero.
pub const DEFAULT_CONCURRENCY: usize = 4;

/// One file a version needs that is not in place yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingFile {
    pub url: String,
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

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
    let root = game_root.join(LIBRARIES_DIR);
    let mut destinations = std::collections::HashSet::new();
    libraries
        .iter()
        .filter(|library| rules::library_applies(library, target))
        .flat_map(|library| {
            let regular = library
                .download()
                .map(|artifact| (root.join(&artifact.path), artifact));
            let classifier = classifier_download(library, target).map(|artifact| {
                let maven_path = root.join(&artifact.path);
                let path = artifact
                    .sha1
                    .as_deref()
                    .and_then(|sha1| axlauncher_storage::native_archive(game_root, sha1))
                    .filter(|_| needs_download(&maven_path, artifact.size))
                    .unwrap_or(maven_path);
                (path, artifact.clone())
            });
            regular.into_iter().chain(classifier)
        })
        .filter_map(|(path, artifact)| {
            (needs_download(&path, artifact.size) && destinations.insert(path.clone())).then(|| {
                MissingFile {
                    // An artifact without a URL stays in the plan, so the download
                    // reports it instead of the plan hiding it.
                    url: artifact.url.clone().unwrap_or_default(),
                    path,
                    sha1: artifact.sha1.clone(),
                    size: artifact.size,
                }
            })
        })
        .collect()
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
        size: client.file.size,
    })
}

/// The asset objects `index` names that are absent or the wrong size under
/// `game_root`.
pub fn missing_assets(game_root: &Path, index: &AssetIndexDocument) -> Vec<MissingFile> {
    let root = game_root.join(ASSETS_DIR).join(OBJECTS_DIR);
    index
        .objects
        .values()
        .filter_map(|object| {
            let prefix = object.hash.get(..OBJECT_PREFIX_LEN)?;
            let hash = &object.hash;
            let path = root.join(prefix).join(hash);
            needs_download(&path, Some(object.size)).then(|| MissingFile {
                url: format!("{ASSET_OBJECT_BASE}/{prefix}/{hash}"),
                path,
                sha1: Some(hash.clone()),
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
    let existing = fs::metadata(path).map(|metadata| metadata.len()).ok();
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

pub(crate) fn materialise_flat_at(
    game_root: &Path,
    root: &Path,
    index: &AssetIndexDocument,
) -> Vec<String> {
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

/// Copies one object into the flat tree, creating its parent.
fn copy_object(source: &Path, destination: &Path) -> std::io::Result<()> {
    let bytes = fs::read(source)?;
    write_atomic(destination, &bytes)
}

/// What happened to one file in a download run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome {
    pub path: PathBuf,
    pub result: Result<(), String>,
}

/// Fetches every file in `files`, writing only those whose SHA-1 matches.
///
/// At most `concurrency` requests are in flight at once; zero means
/// [`DEFAULT_CONCURRENCY`]. Outcomes arrive in completion order.
pub async fn download_all(
    network: &impl Network,
    files: &[MissingFile],
    concurrency: usize,
) -> Vec<DownloadOutcome> {
    let limit = if concurrency == 0 {
        DEFAULT_CONCURRENCY
    } else {
        concurrency
    };
    stream::iter(files)
        .map(|file| download(network, file))
        .buffer_unordered(limit)
        .collect()
        .await
}

/// The outcome of fetching and writing one file.
async fn download(network: &impl Network, file: &MissingFile) -> DownloadOutcome {
    DownloadOutcome {
        path: file.path.clone(),
        result: fetch_and_write(network, file).await,
    }
}

async fn fetch_and_write(network: &impl Network, file: &MissingFile) -> Result<(), String> {
    let bytes = network.fetch(&file.url).await.map_err(reason)?;
    if let Some(expected) = &file.sha1
        && !verify(&bytes, expected)
    {
        return Err(format!("SHA-1 mismatch for {}", file.path.display()));
    }
    write_atomic(&file.path, &bytes)
        .map_err(|error| format!("cannot write {}: {error}", file.path.display()))
}

/// The text of a fetch error, with the retry advice dropped.
fn reason(error: FetchError) -> String {
    match error {
        FetchError::Retryable(reason) | FetchError::Fatal(reason) => reason,
        rejected @ FetchError::Rejected { .. } => rejected.to_string(),
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/install/src/assets/tests.rs"]
mod tests;
