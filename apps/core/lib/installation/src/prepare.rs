// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Getting a version's files onto disk before a launch.
//!
//! The steps are the ones a fresh install needs: acquire the asset index if it
//! is missing, list everything absent, download and verify it, and report what
//! landed. Starting the game is a separate call, so a caller can show progress
//! between the two.

use axlauncher_download_spi::FileDownloader;
use axlauncher_install::{
    assets::{AssetIndexDocument, missing_flat_copies},
    prepare::{PrepareError, Prepared, Progress, index_error, outstanding, parse_index, summarise},
};
use axlauncher_launch::Target;
use axlauncher_launch::version::Version;
use axlauncher_network_spi::{FetchKind, Network};
use axlauncher_storage::{AssetPath, GameDir};
use futures::StreamExt;

/// Ensures the asset index document exists under `game_root`.
///
/// An index already on disk is read as it is: it is content-addressed by the
/// version document, so a second copy could only be the same bytes.
pub async fn ensure_asset_index(
    network: &impl Network,
    game_root: &GameDir,
    version: &Version,
) -> Result<AssetIndexDocument, PrepareError> {
    let Some(index) = version.asset_index.clone() else {
        return Ok(AssetIndexDocument::default());
    };

    let path = game_root.asset_path(AssetPath::Index(&index.id));
    match std::fs::read(&path) {
        Ok(bytes) => return parse_index(&index, &bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(index_error(&index, error)),
    }
    let (document, bytes) = network
        .fetch_with(&index.url, FetchKind::Metadata, |response| {
            let bytes = response.map_err(|source| PrepareError::AssetIndex {
                id: index.id.clone(),
                source,
            })?;
            Ok((parse_index(&index, &bytes)?, bytes))
        })
        .await?;
    axlauncher_storage::write_atomic(&path, &bytes).map_err(|error| index_error(&index, error))?;
    Ok(document)
}

/// Downloads everything `version` needs that is not on disk.
pub async fn prepare(
    network: &impl Network,
    downloader: &impl FileDownloader,
    game_root: &GameDir,
    version: &Version,
    target: &Target,
    concurrency: usize,
) -> Result<Prepared, PrepareError> {
    prepare_reporting(
        network,
        downloader,
        game_root,
        version,
        target,
        concurrency,
        |_| {},
    )
    .await
}

pub(crate) async fn prepare_reporting(
    network: &impl Network,
    downloader: &impl FileDownloader,
    game_root: &GameDir,
    version: &Version,
    target: &Target,
    concurrency: usize,
    mut report: impl FnMut(Progress),
) -> Result<Prepared, PrepareError> {
    report(Progress::CheckingFiles);
    let index = ensure_asset_index(network, game_root, version).await?;
    let files = outstanding(game_root, version, &index, target);
    let total = files.len();
    if total > 0 {
        report(Progress::Downloading {
            processed: 0,
            total,
        });
    }
    let mut processed = 0;
    let outcomes = downloader
        .batch(&files, concurrency)
        .inspect(|_| {
            processed += 1;
            report(Progress::Downloading { processed, total });
        })
        .collect::<Vec<_>>()
        .await;
    report(Progress::Finishing);
    let mut prepared = summarise(&outcomes);

    // The flat tree copies from the object store, so it is built after the
    // objects land. A version whose index asks for none does nothing here.
    if let Some(index_id) = version.asset_index.as_ref().map(|index| index.id.as_str()) {
        prepared
            .failures
            .extend(materialise_flat(game_root, index_id, &index));
    }
    Ok(prepared)
}

fn materialise_flat(game: &GameDir, id: &str, index: &AssetIndexDocument) -> Vec<String> {
    const MAX_FLAT_FAILURES: usize = 8;
    let mut failures = Vec::new();
    for (destination, hash) in missing_flat_copies(game, id, index) {
        let Some(source) = game.asset_object(hash) else {
            continue;
        };
        let result = std::fs::read(source)
            .and_then(|bytes| axlauncher_storage::write_atomic(&destination, &bytes));
        if let Err(error) = result
            && failures.len() < MAX_FLAT_FAILURES
        {
            failures.push(format!("{}: {error}", destination.display()));
        }
    }
    failures
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/installation/src/prepare.rs"]
mod tests;
