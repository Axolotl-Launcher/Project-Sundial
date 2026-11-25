// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Published artifact versions from the Forge and NeoForge Maven indexes.

use axlauncher_spi::{FetchKind, LoaderError, LoaderKind, Network};
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

pub(super) async fn fetch_versions<T>(
    network: &impl Network,
    loader: LoaderKind,
    repository: &str,
    accept: impl Fn(Vec<String>) -> Result<T, LoaderError>,
) -> Result<T, LoaderError> {
    let url = format!("{repository}/maven-metadata.xml");
    network
        .fetch_with(&url, FetchKind::UnverifiedMetadata, |response| {
            let bytes = response.map_err(|source| LoaderError::Unavailable { loader, source })?;
            let document: Metadata =
                quick_xml::de::from_reader(bytes.as_slice()).map_err(|error| {
                    LoaderError::Malformed {
                        loader,
                        reason: format!("an invalid Maven version list: {error}"),
                    }
                })?;
            accept(document.versioning.versions.entries)
        })
        .await
}
