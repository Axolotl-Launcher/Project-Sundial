// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Forge: a Maven catalog for browsing and promotions for the default build.
//!
//! Forge publishes two loader versions per game version, `latest` and
//! `recommended`, in one small JSON document. Everything else about an install
//! comes from its installer jar. The Maven catalog also lists builds without a
//! promotion, including historical builds.

use std::collections::BTreeMap;
use std::time::Duration;

use axlauncher_mc_version::MavenTarget;
use axlauncher_network_spi::{FetchError, FetchKind, Network};
use axlauncher_spi::{
    LoaderError, LoaderKind, LoaderProfile, LoaderRequest, LoaderVersion, ModLoader,
};
use serde::Deserialize;

use crate::{installer, maven_metadata};

/// Where Forge lists the versions it recommends.
const PROMOTIONS: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";

/// Limit how long optional recommendations can hold up the Maven listing.
const CATALOG_PROMOTIONS_TIMEOUT: Duration = Duration::from_secs(3);
const INSTALLER_NOT_FOUND: u16 = 404;

/// The channels a promotion entry can name.
const RECOMMENDED: &str = "recommended";
const LATEST: &str = "latest";

#[derive(Deserialize)]
struct Promotions {
    promos: BTreeMap<String, String>,
}

impl Promotions {
    fn version(&self, game_version: &str, channel: &str) -> Option<&str> {
        self.promos
            .get(&format!("{game_version}-{channel}"))
            .map(String::as_str)
    }
}

/// Forge's published loader versions.
#[derive(Debug, Clone)]
pub struct ForgeApi<N: Network> {
    network: N,
}

impl<N: Network> ForgeApi<N> {
    pub fn new(network: N) -> Self {
        Self { network }
    }
}

impl<N: Network> ModLoader for ForgeApi<N> {
    fn kind(&self) -> LoaderKind {
        LoaderKind::Forge
    }

    async fn versions(&self, game_version: &str) -> Result<Vec<LoaderVersion>, LoaderError> {
        let target = maven_metadata::target(self.kind(), game_version)?;
        let (builds, recommended) = tokio::join!(
            maven_metadata::fetch_builds(&self.network, &target, Ok),
            tokio::time::timeout(
                CATALOG_PROMOTIONS_TIMEOUT,
                self.promoted_version(game_version, &[RECOMMENDED]),
            ),
        );
        let mut versions = builds?.into_offers();
        if let Ok(Ok(recommended)) = recommended
            && let Some(entry) = versions
                .iter_mut()
                .find(|entry| entry.version == recommended)
        {
            entry.stable = Some(true);
        }
        Ok(versions)
    }

    async fn profile(&self, request: LoaderRequest) -> Result<LoaderProfile, LoaderError> {
        let target = maven_metadata::target(self.kind(), &request.game_version)?;
        let coordinate = match request.loader_version.as_deref() {
            Some(requested) if !requested.trim().is_empty() => {
                self.named_coordinate(&target, requested).await?
            }
            _ => self.default_coordinate(&target).await?,
        };
        self.install(&target, &coordinate).await
    }
}

impl<N: Network> ForgeApi<N> {
    async fn promoted_version(
        &self,
        game_version: &str,
        channels: &[&str],
    ) -> Result<String, LoaderError> {
        self.network
            .fetch_with(PROMOTIONS, FetchKind::UnverifiedMetadata, |response| {
                promoted_version(game_version, channels, response)
            })
            .await
    }

    async fn default_coordinate(&self, target: &MavenTarget<'_>) -> Result<String, LoaderError> {
        let promotion = match self
            .promoted_version(target.game(), &[RECOMMENDED, LATEST])
            .await
        {
            Ok(promotion) => Some(promotion),
            Err(LoaderError::NoVersions { .. }) => None,
            Err(error) => return Err(error),
        };
        maven_metadata::fetch_builds(&self.network, target, |builds| {
            promotion
                .as_deref()
                .and_then(|version| builds.find(version))
                .or_else(|| builds.preferred())
                .map(|build| build.coordinate().to_owned())
                .ok_or_else(|| no_versions(target.game()))
        })
        .await
    }

    async fn named_coordinate(
        &self,
        target: &MavenTarget<'_>,
        requested: &str,
    ) -> Result<String, LoaderError> {
        match maven_metadata::resolve(&self.network, target, Some(requested)).await {
            Err(LoaderError::Unavailable { .. } | LoaderError::Malformed { .. }) => {
                Ok(requested.to_owned())
            }
            result => result,
        }
    }

    async fn install(
        &self,
        target: &MavenTarget<'_>,
        requested: &str,
    ) -> Result<LoaderProfile, LoaderError> {
        let (primary, alternate) =
            target
                .forge_coordinates(requested)
                .ok_or_else(|| LoaderError::UnknownVersion {
                    loader: self.kind(),
                    version: requested.to_owned(),
                })?;
        let result = self.fetch_installer(target, &primary).await;
        if let Err(LoaderError::Unavailable {
            source:
                FetchError::Rejected {
                    status: INSTALLER_NOT_FOUND,
                    ..
                },
            ..
        }) = &result
            && let Some(alternate) = alternate
        {
            return self.fetch_installer(target, &alternate).await;
        }
        result
    }

    async fn fetch_installer(
        &self,
        target: &MavenTarget<'_>,
        coordinate: &str,
    ) -> Result<LoaderProfile, LoaderError> {
        let url = maven_metadata::installer_url(target.artifact(), coordinate);
        installer::fetch(&self.network, self.kind(), &url, target.game()).await
    }
}

fn promoted_version(
    game_version: &str,
    channels: &[&str],
    response: Result<Vec<u8>, FetchError>,
) -> Result<String, LoaderError> {
    let loader = LoaderKind::Forge;
    let bytes = response.map_err(|source| LoaderError::Unavailable { loader, source })?;
    let promotions: Promotions =
        serde_json::from_slice(&bytes).map_err(|error| LoaderError::Malformed {
            loader,
            reason: format!("a promotions list that does not parse: {error}"),
        })?;
    channels
        .iter()
        .find_map(|channel| promotions.version(game_version, channel))
        .map(str::to_owned)
        .ok_or_else(|| no_versions(game_version))
}

fn no_versions(game_version: &str) -> LoaderError {
    LoaderError::NoVersions {
        loader: LoaderKind::Forge,
        game_version: game_version.to_owned(),
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/loader/src/forge.rs"]
mod tests;
