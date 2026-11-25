// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! NeoForge: a Maven metadata document and an installer jar.
//!
//! NeoForge publishes its loaders to a Maven repository, so the version list is
//! the repository's metadata. A loader version names the game version it is for
//! by its own prefix: `20.2.93` is for Minecraft `1.20.2`, `21.11.4` for
//! `1.21.11`.

use axlauncher_spi::{
    LoaderError, LoaderKind, LoaderProfile, LoaderRequest, LoaderVersion, ModLoader, Network,
};

use crate::versions::compare;
use crate::{installer, maven_metadata};

/// The repository NeoForge publishes to.
const MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";

/// Where NeoForge published its Minecraft 1.20.1 builds, under a different
/// artifact whose versions carry the game version as a prefix.
const LEGACY_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/forge";

/// The one game version published under [`LEGACY_MAVEN`].
const LEGACY_GAME_VERSION: &str = "1.20.1";

/// NeoForge's published loader versions.
#[derive(Debug, Clone)]
pub struct NeoForgeApi<N: Network> {
    network: N,
}

impl<N: Network> NeoForgeApi<N> {
    pub fn new(network: N) -> Self {
        Self { network }
    }

    /// Where the installer for one loader version lives.
    pub fn installer_url(loader_version: &str) -> String {
        format!("{MAVEN}/{loader_version}/neoforge-{loader_version}-installer.jar")
    }

    /// Where the installer for a Minecraft 1.20.1 build lives.
    fn legacy_installer_url(loader_version: &str) -> String {
        format!("{LEGACY_MAVEN}/{loader_version}/forge-{loader_version}-installer.jar")
    }
}

impl<N: Network> ModLoader for NeoForgeApi<N> {
    fn kind(&self) -> LoaderKind {
        LoaderKind::NeoForge
    }

    async fn versions(&self, game_version: &str) -> Result<Vec<LoaderVersion>, LoaderError> {
        self.offers(game_version, None).await
    }

    async fn profile(&self, request: LoaderRequest) -> Result<LoaderProfile, LoaderError> {
        let offers = self
            .offers(&request.game_version, request.loader_version.as_deref())
            .await?;
        let chosen = offers
            .iter()
            .find(|offer| offer.stable == Some(true))
            .unwrap_or(&offers[0]);

        let url = if request.game_version == LEGACY_GAME_VERSION {
            Self::legacy_installer_url(&chosen.version)
        } else {
            Self::installer_url(&chosen.version)
        };
        installer::fetch(&self.network, self.kind(), &url, &request.game_version).await
    }
}

impl<N: Network> NeoForgeApi<N> {
    /// Reads the repository that publishes builds for the requested game line.
    async fn offers(
        &self,
        game_version: &str,
        requested: Option<&str>,
    ) -> Result<Vec<LoaderVersion>, LoaderError> {
        let repository = if game_version == LEGACY_GAME_VERSION {
            LEGACY_MAVEN
        } else {
            MAVEN
        };
        maven_metadata::fetch_versions(&self.network, self.kind(), repository, |versions| {
            select_offers(game_version, requested, versions)
        })
        .await
    }
}

fn select_offers(
    game_version: &str,
    requested: Option<&str>,
    versions: Vec<String>,
) -> Result<Vec<LoaderVersion>, LoaderError> {
    let loader = LoaderKind::NeoForge;
    let mut versions: Vec<_> = versions
        .into_iter()
        .filter(|version| matches_game(version, game_version))
        .map(|version| LoaderVersion {
            stable: Some(!is_prerelease(&version)),
            version,
        })
        .collect();
    if versions.is_empty() {
        return Err(LoaderError::NoVersions {
            loader,
            game_version: game_version.into(),
        });
    }
    if let Some(requested) = requested {
        return versions
            .into_iter()
            .find(|offer| offer.version == requested)
            .map(|offer| vec![offer])
            .ok_or_else(|| LoaderError::UnknownVersion {
                loader,
                version: requested.into(),
            });
    }
    versions.sort_by(|left, right| compare(&right.version, &left.version));
    versions.dedup_by(|left, right| left.version == right.version);
    Ok(versions)
}

/// NeoForge encodes the game version before its build counter. From 26.x the
/// full game version is included, with a zero for an omitted patch and build
/// metadata identifying a particular Minecraft snapshot or pre-release.
fn matches_game(version: &str, game_version: &str) -> bool {
    if game_version == LEGACY_GAME_VERSION {
        return version.starts_with("1.20.1-");
    }
    let (build, selector) = version
        .split_once('+')
        .map_or((version, None), |(build, tag)| (build, Some(tag)));
    let build = build.split('-').next().unwrap_or_default();
    let Some((game, _counter)) = build.rsplit_once('.') else {
        return false;
    };
    if let Some(snapshot) = game.strip_prefix("0.") {
        return snapshot == game_version;
    }
    let (base, qualifier) = game_version
        .split_once('-')
        .map_or((game_version, None), |(base, tag)| (base, Some(tag)));
    let qualifier = qualifier.map(|tag| {
        tag.strip_prefix("release-candidate-")
            .map_or_else(|| tag.to_owned(), |number| format!("rc-{number}"))
    });
    if selector != qualifier.as_deref() {
        return false;
    }
    let base = base.strip_prefix("1.").unwrap_or(base);
    base == game || game.strip_suffix(".0") == Some(base)
}

/// Whether a NeoForge version names a pre-release.
fn is_prerelease(version: &str) -> bool {
    let lower = version.to_ascii_lowercase();
    ["-beta", "-alpha", "-rc", "-pre", "+", "snapshot"]
        .iter()
        .any(|tag| lower.contains(tag))
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/loader/src/neoforge.rs"]
mod tests;
