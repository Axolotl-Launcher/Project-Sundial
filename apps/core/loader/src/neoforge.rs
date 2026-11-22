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
}

impl<N: Network> ModLoader for NeoForgeApi<N> {
    fn kind(&self) -> LoaderKind {
        LoaderKind::NeoForge
    }

    async fn versions(&self, game_version: &str) -> Result<Vec<LoaderVersion>, LoaderError> {
        let url = format!("{MAVEN}/maven-metadata.xml");
        let bytes = self
            .network
            .fetch(&url)
            .await
            .map_err(|source| LoaderError::Unavailable {
                loader: self.kind(),
                source,
            })?;
        let metadata = String::from_utf8_lossy(&bytes);

        let prefix = format!("{}.", game_prefix(game_version));
        let mut versions: Vec<LoaderVersion> = versions_in(&metadata)
            .into_iter()
            .filter(|version| version.starts_with(&prefix))
            .map(|version| LoaderVersion {
                version,
                stable: None,
            })
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
            None => &offers[0],
        };

        let url = Self::installer_url(&chosen.version);
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

/// The prefix a NeoForge version carries for a game version.
///
/// Minecraft `1.20.2` and NeoForge `20.2.x`; Minecraft `26.1` and NeoForge
/// `26.1.x`. Dropping the leading `1.` is the whole rule.
fn game_prefix(game_version: &str) -> &str {
    game_version.strip_prefix("1.").unwrap_or(game_version)
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
