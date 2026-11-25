// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Version catalogs for the desktop's profile form.

use std::future::Future;
use std::time::Duration;

use axlauncher_install::vanilla;
use axlauncher_spi::{LoaderError, LoaderKind, LoaderVersion, ModLoader};

use super::{on_worker, sources::DownloadOptions};

pub use axlauncher_loader::supported_kinds as supported_loaders;

const CATALOG_TIMEOUT: Duration = Duration::from_secs(15);

/// The Minecraft catalog. Call from a worker rather than the UI thread.
pub fn minecraft_versions(options: DownloadOptions) -> Result<vanilla::MinecraftCatalog, String> {
    let network = options.network()?;
    catalog_request(vanilla::versions(&network))
}

/// The versions a supported loader offers for this game, possibly none.
pub fn loader_versions(
    options: DownloadOptions,
    kind: LoaderKind,
    game_version: &str,
) -> Result<Vec<LoaderVersion>, String> {
    catalog_request(async {
        let provider = axlauncher_loader::provider(options.network()?, kind)
            .map_err(|error| error.to_string())?;
        match provider.versions(game_version).await {
            Ok(versions) => Ok(versions),
            Err(LoaderError::NoVersions { .. }) => Ok(Vec::new()),
            Err(error) => Err(error.to_string()),
        }
    })
}

fn catalog_request<T>(request: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    on_worker(async {
        tokio::time::timeout(CATALOG_TIMEOUT, request)
            .await
            .map_err(|_| "The request timed out.".to_owned())?
    })
    .map_err(|error| error.to_string())?
}
