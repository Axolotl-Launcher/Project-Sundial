// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The official Minecraft version catalog used by both browsing and installation.

use axlauncher_network_spi::{FetchKind, Network};
use serde::Deserialize;

pub(super) const CATALOG: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// Mojang's game versions in published order, newest first.
#[derive(Debug, Deserialize)]
pub struct MinecraftCatalog {
    latest: Latest,
    versions: Vec<MinecraftVersion>,
}

#[derive(Debug, Deserialize)]
struct Latest {
    release: String,
}

/// A selectable Minecraft version and the manifest that installs it.
#[derive(Debug, Deserialize)]
pub struct MinecraftVersion {
    id: String,
    #[serde(rename = "type")]
    manifest_type: String,
    pub(super) url: String,
    pub(super) sha1: String,
}

impl MinecraftCatalog {
    pub fn latest_release(&self) -> &str {
        &self.latest.release
    }

    /// Each selectable version as its id and manifest type, in published order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.versions
            .iter()
            .map(|version| (version.id(), version.manifest_type()))
    }

    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }

    fn into_version(self, id: &str) -> Option<MinecraftVersion> {
        self.versions.into_iter().find(|entry| entry.id == id)
    }
}

impl MinecraftVersion {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn manifest_type(&self) -> &str {
        &self.manifest_type
    }
}

/// Fetches Mojang's catalog without downloading any game files.
pub async fn versions(network: &impl Network) -> Result<MinecraftCatalog, String> {
    fetch_catalog(network, Ok).await
}

pub(super) async fn find_version(
    network: &impl Network,
    id: &str,
) -> Result<MinecraftVersion, String> {
    fetch_catalog(network, |catalog| {
        catalog
            .into_version(id)
            .ok_or_else(|| format!("Minecraft {id} is not in the Mojang version list."))
    })
    .await
}

async fn fetch_catalog<T>(
    network: &impl Network,
    accept: impl Fn(MinecraftCatalog) -> Result<T, String>,
) -> Result<T, String> {
    network
        .fetch_with(CATALOG, FetchKind::UnverifiedMetadata, |response| {
            let bytes = response
                .map_err(|error| format!("Cannot fetch the Mojang version list: {error}"))?;
            let catalog = serde_json::from_slice(&bytes)
                .map_err(|error| format!("Cannot read the Mojang version list: {error}"))?;
            accept(catalog)
        })
        .await
}
