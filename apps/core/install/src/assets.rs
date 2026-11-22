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

use axlauncher_launch::rules::{self, Target};
use axlauncher_launch::version::Library;
use axlauncher_network::verify;
use axlauncher_spi::{FetchError, Network};
use axlauncher_storage::write_atomic;
use futures::stream::{self, StreamExt};
use serde::Deserialize;

/// The directory under the game root that holds library artifacts.
const LIBRARIES_DIR: &str = "libraries";

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

/// The asset index document, reduced to the object map an install reads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AssetIndexDocument {
    pub objects: BTreeMap<String, AssetObject>,
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
pub fn missing_libraries(
    game_root: &Path,
    libraries: &[Library],
    target: &Target,
) -> Vec<MissingFile> {
    let root = game_root.join(LIBRARIES_DIR);
    libraries
        .iter()
        .filter(|library| rules::library_applies(library, target))
        .filter_map(|library| library.downloads.as_ref()?.artifact.as_ref())
        .filter_map(|artifact| {
            let path = root.join(&artifact.path);
            needs_download(&path, artifact.size).then(|| MissingFile {
                // An artifact without a URL stays in the plan, so the download
                // reports it instead of the plan hiding it.
                url: artifact.url.clone().unwrap_or_default(),
                path,
                sha1: artifact.sha1.clone(),
                size: artifact.size,
            })
        })
        .collect()
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
    }
}

#[cfg(test)]
mod tests;
