// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! NeoForge: a Maven metadata document and an installer jar.
//!
//! Minecraft version conventions select the repository and interpret its
//! listing. This provider owns the network requests and installer document.

use axlauncher_spi::{
    LoaderError, LoaderKind, LoaderProfile, LoaderRequest, LoaderVersion, ModLoader, Network,
};

use crate::{installer, maven_metadata};

/// NeoForge's published loader versions.
#[derive(Debug, Clone)]
pub struct NeoForgeApi<N: Network> {
    network: N,
}

impl<N: Network> NeoForgeApi<N> {
    pub fn new(network: N) -> Self {
        Self { network }
    }
}

impl<N: Network> ModLoader for NeoForgeApi<N> {
    fn kind(&self) -> LoaderKind {
        LoaderKind::NeoForge
    }

    async fn versions(&self, game_version: &str) -> Result<Vec<LoaderVersion>, LoaderError> {
        let target = maven_metadata::target(self.kind(), game_version)?;
        maven_metadata::fetch_builds(&self.network, &target, |builds| Ok(builds.into_offers()))
            .await
    }

    async fn profile(&self, request: LoaderRequest) -> Result<LoaderProfile, LoaderError> {
        let target = maven_metadata::target(self.kind(), &request.game_version)?;
        let coordinate =
            maven_metadata::resolve(&self.network, &target, request.loader_version.as_deref())
                .await?;
        let url = maven_metadata::installer_url(target.artifact(), &coordinate);
        installer::fetch(&self.network, self.kind(), &url, target.game()).await
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/loader/src/neoforge.rs"]
mod tests;
