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

use axlauncher_spi::{
    FetchError, FetchKind, LoaderError, LoaderKind, LoaderProfile, LoaderRequest, LoaderVersion,
    ModLoader, Network,
};
use serde::Deserialize;

use crate::versions::compare;
use crate::{installer, maven_metadata};

/// Where Forge lists the versions it recommends.
const PROMOTIONS: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";

/// Limit how long optional recommendations can hold up the Maven listing.
const CATALOG_PROMOTIONS_TIMEOUT: Duration = Duration::from_secs(3);

/// The repository the installers are published to.
const MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";

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

fn loader_version<'a>(coordinate: &'a str, prefix: &str, repeated_game: &str) -> Option<&'a str> {
    let version = coordinate.strip_prefix(prefix)?;
    // A longer Minecraft version can share the prefix (such as 26.1 and
    // 26.1-snapshot-1). A Forge build itself always starts with a digit.
    if !version.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    Some(version.strip_suffix(repeated_game).unwrap_or(version))
}

impl<N: Network> ModLoader for ForgeApi<N> {
    fn kind(&self) -> LoaderKind {
        LoaderKind::Forge
    }

    async fn versions(&self, game_version: &str) -> Result<Vec<LoaderVersion>, LoaderError> {
        let (versions, recommended) = tokio::join!(
            self.maven_versions(game_version),
            tokio::time::timeout(
                CATALOG_PROMOTIONS_TIMEOUT,
                self.promoted_version(game_version, &[RECOMMENDED]),
            ),
        );
        let mut versions = versions?;
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
        let chosen = match &request.loader_version {
            Some(requested) if !requested.trim().is_empty() => requested.clone(),
            _ => self.default_version(&request.game_version).await?,
        };
        self.install(&request.game_version, &chosen).await
    }
}

impl<N: Network> ForgeApi<N> {
    async fn maven_versions(&self, game_version: &str) -> Result<Vec<LoaderVersion>, LoaderError> {
        maven_metadata::fetch_versions(&self.network, self.kind(), MAVEN, |coordinates| {
            catalog_offers(game_version, &coordinates)
        })
        .await
    }

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

    async fn default_version(&self, game_version: &str) -> Result<String, LoaderError> {
        match self
            .promoted_version(game_version, &[RECOMMENDED, LATEST])
            .await
        {
            Err(LoaderError::NoVersions { .. }) => self
                .maven_versions(game_version)
                .await
                .map(|mut versions| versions.remove(0).version),
            result => result,
        }
    }

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

fn catalog_offers(
    game_version: &str,
    coordinates: &[String],
) -> Result<Vec<LoaderVersion>, LoaderError> {
    let prefix = format!("{game_version}-");
    let repeated_game = format!("-{game_version}");
    let mut versions: Vec<_> = coordinates
        .iter()
        .filter_map(|coordinate| loader_version(coordinate, &prefix, &repeated_game))
        .map(|version| LoaderVersion {
            version: version.to_owned(),
            stable: None,
        })
        .collect();
    if versions.is_empty() {
        return Err(no_versions(game_version));
    }
    versions.sort_by(|left, right| compare(&right.version, &left.version));
    versions.dedup_by(|left, right| left.version == right.version);
    Ok(versions)
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
