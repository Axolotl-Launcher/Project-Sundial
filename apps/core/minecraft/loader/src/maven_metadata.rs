// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Published artifact versions from the Forge and NeoForge Maven indexes.

use axlauncher_mc_version::{MavenArtifact, MavenBuilds, MavenTarget};
use axlauncher_spi::{FetchKind, LoaderError, LoaderKind, Network};
use serde::Deserialize;

const FORGE: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
const NEOFORGE: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const NEOFORGE_1201: &str = "https://maven.neoforged.net/releases/net/neoforged/forge";

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

pub(super) fn target(loader: LoaderKind, game: &str) -> Result<MavenTarget<'_>, LoaderError> {
    MavenTarget::classify(loader, game).ok_or_else(|| no_versions(loader, game))
}

pub(super) fn installer_url(artifact: MavenArtifact, coordinate: &str) -> String {
    let (root, name) = repository(artifact);
    format!("{root}/{coordinate}/{name}-{coordinate}-installer.jar")
}

fn repository(artifact: MavenArtifact) -> (&'static str, &'static str) {
    match artifact {
        MavenArtifact::Forge => (FORGE, "forge"),
        MavenArtifact::NeoForge => (NEOFORGE, "neoforge"),
        MavenArtifact::NeoForge1201 => (NEOFORGE_1201, "forge"),
    }
}

pub(super) async fn fetch_builds<T>(
    network: &impl Network,
    target: &MavenTarget<'_>,
    accept: impl Fn(MavenBuilds) -> Result<T, LoaderError>,
) -> Result<T, LoaderError> {
    let loader = target.loader();
    let (root, _) = repository(target.artifact());
    let url = format!("{root}/maven-metadata.xml");
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
            let builds = target.builds(document.versioning.versions.entries);
            if builds.is_empty() {
                return Err(no_versions(loader, target.game()));
            }
            accept(builds)
        })
        .await
}

pub(super) async fn resolve(
    network: &impl Network,
    target: &MavenTarget<'_>,
    requested: Option<&str>,
) -> Result<String, LoaderError> {
    fetch_builds(network, target, |builds| {
        let build = if let Some(version) = requested {
            builds
                .find(version)
                .ok_or_else(|| LoaderError::UnknownVersion {
                    loader: target.loader(),
                    version: version.to_owned(),
                })?
        } else {
            builds
                .preferred()
                .ok_or_else(|| no_versions(target.loader(), target.game()))?
        };
        Ok(build.coordinate().to_owned())
    })
    .await
}

fn no_versions(loader: LoaderKind, game: &str) -> LoaderError {
    LoaderError::NoVersions {
        loader,
        game_version: game.to_owned(),
    }
}
