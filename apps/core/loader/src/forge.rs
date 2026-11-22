// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Forge: a promotions list and an installer jar.
//!
//! Forge publishes two loader versions per game version, `latest` and
//! `recommended`, in one small JSON document. Everything else about an install
//! comes from the installer jar those entries name.

use std::collections::BTreeMap;

use axlauncher_spi::{
    LoaderError, LoaderKind, LoaderProfile, LoaderRequest, LoaderVersion, ModLoader, Network,
};
use serde::Deserialize;

use crate::installer;
use crate::versions::compare;

/// Where Forge lists the versions it recommends.
const PROMOTIONS: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";

/// The repository the installers are published to.
const MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";

/// The channels a promotion entry can name.
const RECOMMENDED: &str = "recommended";
const LATEST: &str = "latest";

#[derive(Deserialize)]
struct Promotions {
    promos: BTreeMap<String, String>,
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

    /// Where the installer for one loader version lives.
    pub fn installer_url(game_version: &str, loader_version: &str) -> String {
        format!(
            "{MAVEN}/{game_version}-{loader_version}/forge-{game_version}-{loader_version}-installer.jar"
        )
    }
}

/// The stability a promotion entry states, when it is one this build uses.
fn promotion(key: &str, prefix: &str) -> Option<Option<bool>> {
    match key.strip_prefix(prefix)? {
        RECOMMENDED => Some(Some(true)),
        LATEST => Some(Some(false)),
        _ => None,
    }
}

impl<N: Network> ModLoader for ForgeApi<N> {
    fn kind(&self) -> LoaderKind {
        LoaderKind::Forge
    }

    async fn versions(&self, game_version: &str) -> Result<Vec<LoaderVersion>, LoaderError> {
        let bytes =
            self.network
                .fetch(PROMOTIONS)
                .await
                .map_err(|source| LoaderError::Unavailable {
                    loader: self.kind(),
                    source,
                })?;
        let promotions: Promotions =
            serde_json::from_slice(&bytes).map_err(|error| LoaderError::Malformed {
                loader: self.kind(),
                reason: format!("a promotions list that does not parse: {error}"),
            })?;

        let prefix = format!("{game_version}-");
        let mut found: BTreeMap<String, Option<bool>> = BTreeMap::new();
        let entries: Vec<(String, Option<bool>)> = promotions
            .promos
            .into_iter()
            .filter_map(|(key, version)| promotion(&key, &prefix).map(|stable| (version, stable)))
            .collect();
        for (version, stable) in entries {
            let best = match (found.get(&version), stable) {
                (_, Some(true)) => Some(true),
                (Some(existing), _) => *existing,
                (None, _) => stable,
            };
            found.insert(version, best);
        }

        let mut versions: Vec<LoaderVersion> = found
            .into_iter()
            .map(|(version, stable)| LoaderVersion { version, stable })
            .collect();
        if versions.is_empty() {
            return Err(LoaderError::NoVersions {
                loader: self.kind(),
                game_version: game_version.to_owned(),
            });
        }
        versions.sort_by(|left, right| compare(&right.version, &left.version));
        Ok(versions)
    }

    async fn profile(&self, request: LoaderRequest) -> Result<LoaderProfile, LoaderError> {
        let offers = self.versions(&request.game_version).await?;
        let chosen = match &request.loader_version {
            Some(requested) => offers
                .iter()
                .find(|offer| &offer.version == requested)
                .ok_or_else(|| LoaderError::UnknownVersion {
                    loader: self.kind(),
                    version: requested.clone(),
                })?,
            None => offers
                .iter()
                .find(|offer| offer.stable == Some(true))
                .unwrap_or(&offers[0]),
        };

        let url = Self::installer_url(&request.game_version, &chosen.version);
        let bytes = self
            .network
            .fetch(&url)
            .await
            .map_err(|source| LoaderError::Unavailable {
                loader: self.kind(),
                source,
            })?;
        installer::parse(self.kind(), &bytes, &request.game_version)
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/loader/src/forge.rs"]
mod tests;
