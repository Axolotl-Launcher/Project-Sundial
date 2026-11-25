// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Forge: a promotions list and an installer jar.
//!
//! Forge publishes two loader versions per game version, `latest` and
//! `recommended`, in one small JSON document. Everything else about an install
//! comes from the installer jar those entries name.

use std::collections::BTreeMap;

use axlauncher_spi::{
    FetchError, FetchKind, LoaderError, LoaderKind, LoaderProfile, LoaderRequest, LoaderVersion,
    ModLoader, Network,
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

    /// Where old installers live, whose Maven directory repeats the game
    /// version: `1.7.10-10.13.4.1614-1.7.10`.
    fn legacy_installer_url(game_version: &str, loader_version: &str) -> String {
        format!(
            "{MAVEN}/{game_version}-{loader_version}-{game_version}/forge-{game_version}-{loader_version}-{game_version}-installer.jar"
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
        self.network
            .fetch_with(PROMOTIONS, FetchKind::Metadata, |response| {
                parse_promotions(game_version, response)
            })
            .await
    }

    async fn profile(&self, request: LoaderRequest) -> Result<LoaderProfile, LoaderError> {
        let chosen = match &request.loader_version {
            // The promotions document names only the two recommended builds,
            // and the Maven hosts every published one. A caller that names a
            // version is taken at its word; the download decides whether it
            // exists.
            Some(requested) if !requested.trim().is_empty() => requested.clone(),
            _ => {
                let offers = self.versions(&request.game_version).await?;
                offers
                    .iter()
                    .find(|offer| offer.stable == Some(true))
                    .unwrap_or(&offers[0])
                    .version
                    .clone()
            }
        };
        self.install(&request.game_version, &chosen).await
    }
}

fn parse_promotions(
    game_version: &str,
    response: Result<Vec<u8>, FetchError>,
) -> Result<Vec<LoaderVersion>, LoaderError> {
    let loader = LoaderKind::Forge;
    let bytes = response.map_err(|source| LoaderError::Unavailable { loader, source })?;
    let promotions: Promotions =
        serde_json::from_slice(&bytes).map_err(|error| LoaderError::Malformed {
            loader,
            reason: format!("a promotions list that does not parse: {error}"),
        })?;
    let prefix = format!("{game_version}-");
    let mut found: BTreeMap<String, Option<bool>> = BTreeMap::new();
    let entries = promotions
        .promos
        .into_iter()
        .filter_map(|(key, version)| promotion(&key, &prefix).map(|stable| (version, stable)));
    for (version, stable) in entries {
        let best = match (found.get(&version), stable) {
            (_, Some(true)) => Some(true),
            (Some(existing), _) => *existing,
            (None, _) => stable,
        };
        found.insert(version, best);
    }
    let mut versions: Vec<_> = found
        .into_iter()
        .map(|(version, stable)| LoaderVersion { version, stable })
        .collect();
    if versions.is_empty() {
        return Err(LoaderError::NoVersions {
            loader,
            game_version: game_version.into(),
        });
    }
    versions.sort_by(|left, right| compare(&right.version, &left.version));
    Ok(versions)
}

impl<N: Network> ForgeApi<N> {
    /// Downloads and parses the installer for one loader version.
    async fn install(
        &self,
        game_version: &str,
        loader_version: &str,
    ) -> Result<LoaderProfile, LoaderError> {
        let url = Self::installer_url(game_version, loader_version);
        match installer::fetch(&self.network, self.kind(), &url, game_version).await {
            // Before 1.9 the Maven directory repeats the game version, so the
            // unsuffixed path is a 404 for an installer that does exist.
            Err(LoaderError::Unavailable {
                source: FetchError::Rejected { .. },
                ..
            }) => {
                let legacy = Self::legacy_installer_url(game_version, loader_version);
                installer::fetch(&self.network, self.kind(), &legacy, game_version).await
            }
            result => result,
        }
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/loader/src/forge.rs"]
mod tests;
