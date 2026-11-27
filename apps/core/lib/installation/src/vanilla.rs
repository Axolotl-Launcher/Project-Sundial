// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Acquisition and preparation of official Minecraft client versions.

use std::io;

use axlauncher_download_spi::FileDownloader;
use axlauncher_install::{
    prepare::{Prepared, Progress},
    vanilla::{check_id, find_version, vanilla_client},
};
use axlauncher_launch::Target;
use axlauncher_launch::version::{self, Version, VersionError};
use axlauncher_network::verify;
use axlauncher_network_spi::{FetchKind, Network};
use axlauncher_storage::{GameDir, VersionPath, write_atomic};

use crate::prepare::prepare_reporting;

/// Installation uses the shared game root and reuses files already in place.
///
/// The caller must hold the game tree's install lock for the complete operation.
pub async fn install(
    network: &impl Network,
    downloader: &impl FileDownloader,
    game_root: &GameDir,
    id: &str,
    target: &Target,
    concurrency: usize,
    mut report: impl FnMut(Progress),
) -> Result<Prepared, String> {
    check_id(id)?;
    report(Progress::Version);
    let version = ensure_version(network, game_root, id).await?;
    prepare_reporting(
        network,
        downloader,
        game_root,
        &version,
        target,
        concurrency,
        report,
    )
    .await
    .map_err(|error| error.to_string())
}

pub(crate) async fn ensure_version(
    network: &impl Network,
    game_root: &GameDir,
    id: &str,
) -> Result<Version, String> {
    check_id(id)?;
    match version::load(game_root, id) {
        Ok(version) => return vanilla_client(version, id),
        Err(VersionError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.to_string()),
    }

    let entry = find_version(network, id).await?;
    let (version, bytes) = network
        .fetch_with(entry.url(), FetchKind::Metadata, |response| {
            let bytes = response
                .map_err(|error| format!("Cannot fetch the Minecraft {id} manifest: {error}"))?;
            if !verify(&bytes, entry.sha1()) {
                return Err(format!(
                    "The Minecraft {id} manifest failed its SHA-1 check."
                ));
            }
            let version = serde_json::from_slice(&bytes)
                .map_err(|error| format!("Cannot read the Minecraft {id} manifest: {error}"))?;
            Ok((vanilla_client(version, id)?, bytes))
        })
        .await?;
    let path = game_root.version_path(id, VersionPath::Document);
    write_atomic(&path, &bytes)
        .map_err(|error| format!("Cannot write {}: {error}", path.display()))?;
    Ok(version)
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/installation/src/vanilla.rs"]
mod tests;
