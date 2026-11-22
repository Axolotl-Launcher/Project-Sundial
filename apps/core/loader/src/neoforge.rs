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

use crate::installer;
use crate::versions::compare;

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
        let prefix = format!("{}.", game_prefix(game_version));
        let mut versions = self.versions_under(MAVEN, game_version, &prefix).await?;
        if versions.is_empty() && game_version == LEGACY_GAME_VERSION {
            // These builds were published before the artifact was renamed; they
            // name the game version as a prefix instead.
            let prefix = format!("{game_version}-");
            versions = self
                .versions_under(LEGACY_MAVEN, game_version, &prefix)
                .await?;
        }
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

        // A version that names the game version is one of the 1.20.1 builds,
        // which live under the older artifact.
        let url = if chosen
            .version
            .starts_with(&format!("{}-", request.game_version))
        {
            Self::legacy_installer_url(&chosen.version)
        } else {
            Self::installer_url(&chosen.version)
        };
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

impl<N: Network> NeoForgeApi<N> {
    /// The versions one artifact's metadata lists under `prefix`.
    async fn versions_under(
        &self,
        maven: &str,
        game_version: &str,
        prefix: &str,
    ) -> Result<Vec<LoaderVersion>, LoaderError> {
        let url = format!("{maven}/maven-metadata.xml");
        let bytes = self
            .network
            .fetch(&url)
            .await
            .map_err(|source| LoaderError::Unavailable {
                loader: self.kind(),
                source,
            })?;
        let metadata = String::from_utf8_lossy(&bytes);

        // The modern artifact spells a game version as a dotted line (`21.0.x`
        // for 1.21); the 1.20.1 artifact prefixes the game version itself
        // (`1.20.1-47.x`). `prefix` is the artifact's spelling, so the modern
        // filter only applies when it is the dotted one.
        let modern = prefix.ends_with('.');
        Ok(versions_in(&metadata)
            .into_iter()
            .filter(|version| version.starts_with(prefix))
            .filter(|version| !modern || matches_game(version, game_version, prefix))
            .map(|version| {
                let stable = Some(!is_prerelease(&version));
                LoaderVersion { version, stable }
            })
            .collect())
    }
}

/// The prefix a NeoForge version carries for a game version.
///
/// Minecraft `1.20.2` and NeoForge `20.2.x`; Minecraft `26.1` and NeoForge
/// `26.1.x`. Dropping the leading `1.` is the whole rule.
fn game_prefix(game_version: &str) -> &str {
    game_version.strip_prefix("1.").unwrap_or(game_version)
}

/// Whether `version` is a NeoForge build for `game_version`.
///
/// A build for Minecraft `1.21` is spelled `21.0.x`, so the first two numeric
/// components name the game version and the third is the loader's own build
/// number. Minecraft `1.21.1` is the separate line `21.1.x`. A bare prefix
/// match would let `21.1.x` answer for `1.21`, whose own builds are `21.0.x`.
fn matches_game(version: &str, game_version: &str, prefix: &str) -> bool {
    let Some(rest) = version.strip_prefix(prefix) else {
        return false;
    };
    // `1.21.1` -> `21.1.`: every build under that line is for this game. A
    // `1.21` build is `21.0.x`, so any other second component belongs to a
    // patch release.
    if game_version.split('.').count() == 3 {
        return true;
    }
    let build = rest.split('.').next().unwrap_or_default();
    build == "0"
}

/// Whether a NeoForge version names a pre-release.
fn is_prerelease(version: &str) -> bool {
    let lower = version.to_ascii_lowercase();
    ["-beta", "alpha", "-pre", "snapshot"]
        .iter()
        .any(|tag| lower.contains(tag))
}

/// The versions a Maven metadata document lists.
fn versions_in(metadata: &str) -> Vec<String> {
    const OPEN: &str = "<version>";
    const CLOSE: &str = "</version>";

    let mut versions = Vec::new();
    let mut rest = metadata;
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start + OPEN.len()..];
        let Some(end) = after.find(CLOSE) else {
            break;
        };
        versions.push(after[..end].trim().to_string());
        rest = &after[end + CLOSE.len()..];
    }
    versions
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/loader/src/neoforge.rs"]
mod tests;
