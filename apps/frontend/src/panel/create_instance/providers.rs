// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Which loaders this build can ask for a version list, and how it asks.
//!
//! A creation form offers only a loader whose provider is wired here. A kind
//! this build cannot serve is not offered at all: a profile naming it would be
//! startable and then fail at the end of a download.

use axlauncher_core::loader::{ForgeApi, LoaderApi, NeoForgeApi};
use axlauncher_core::network::ReqwestNetwork;
use axlauncher_core::spi::{LoaderError, LoaderKind, LoaderVersion, ModLoader};

/// Every loader a new profile can name, in the order the form lists them.
///
/// OptiFine is absent because no provider implements it in this build.
pub(super) const KINDS: [LoaderKind; 5] = [
    LoaderKind::Fabric,
    LoaderKind::LegacyFabric,
    LoaderKind::Quilt,
    LoaderKind::Forge,
    LoaderKind::NeoForge,
];

/// The loader versions `kind` publishes for `game_version`, newest first.
///
/// Each provider is its own type, so the dispatch is over concrete values
/// rather than a trait object, matching how an install selects one.
pub(super) async fn versions(
    network: &ReqwestNetwork,
    kind: LoaderKind,
    game_version: &str,
) -> Result<Vec<LoaderVersion>, LoaderError> {
    match kind {
        LoaderKind::Fabric => {
            LoaderApi::fabric(network.clone())
                .versions(game_version)
                .await
        }
        LoaderKind::LegacyFabric => {
            LoaderApi::legacy_fabric(network.clone())
                .versions(game_version)
                .await
        }
        LoaderKind::Quilt => {
            LoaderApi::quilt(network.clone())
                .versions(game_version)
                .await
        }
        LoaderKind::Forge => ForgeApi::new(network.clone()).versions(game_version).await,
        LoaderKind::NeoForge => {
            NeoForgeApi::new(network.clone())
                .versions(game_version)
                .await
        }
        LoaderKind::OptiFine => Err(LoaderError::Unavailable {
            loader: LoaderKind::OptiFine,
            source: axlauncher_core::spi::FetchError::Fatal(
                "OptiFine has no provider in this build".into(),
            ),
        }),
    }
}

#[cfg(test)]
#[path = "../../../../../tests/apps/frontend/src/panel/create_instance/providers.rs"]
mod tests;
