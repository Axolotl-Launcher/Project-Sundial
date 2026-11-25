// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Published artifact versions from the Forge and NeoForge Maven indexes.

use axlauncher_spi::{LoaderError, LoaderKind, Network};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename = "metadata")]
struct Metadata {
    versioning: Versioning,
}

#[derive(Deserialize)]
struct Versioning {
    versions: Versions,
}

#[derive(Deserialize)]
struct Versions {
    #[serde(rename = "version", default)]
    entries: Vec<String>,
}

pub(super) async fn fetch_versions(
    network: &impl Network,
    loader: LoaderKind,
    repository: &str,
) -> Result<Vec<String>, LoaderError> {
    let url = format!("{repository}/maven-metadata.xml");
    let bytes = network
        .fetch(&url)
        .await
        .map_err(|source| LoaderError::Unavailable { loader, source })?;
    let document: Metadata =
        quick_xml::de::from_reader(bytes.as_slice()).map_err(|error| LoaderError::Malformed {
            loader,
            reason: format!("an invalid Maven version list: {error}"),
        })?;
    Ok(document.versioning.versions.entries)
}
