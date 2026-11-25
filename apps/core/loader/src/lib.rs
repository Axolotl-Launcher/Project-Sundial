// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Resolving a mod loader into the version profile an install follows.
//!
//! A loader's metadata service publishes two things a launcher needs: the
//! loader versions it offers for a game version, and a version profile — a
//! document that names the vanilla version in `inheritsFrom` and adds the
//! loader's main class, libraries and arguments. Installing a loader is
//! therefore fetching one document, not running an installer.
//!
//! [`LoaderApi`] serves the loaders whose metadata follows the same shape.
//! A loader with a different API — one that ships an installer archive, say —
//! is a new type implementing [`ModLoader`], not a branch inside this one.

use axlauncher_spi::{
    FetchError, FetchKind, LoaderError, LoaderKind, LoaderProfile, LoaderRequest, LoaderVersion,
    ModLoader, Network,
};
use serde::Deserialize;

mod forge;
mod installer;
mod library;
mod maven_metadata;
mod neoforge;
mod versions;
pub use forge::ForgeApi;
pub use neoforge::NeoForgeApi;

/// The loader kinds that have a version catalog and installation provider.
pub const SUPPORTED_KINDS: [LoaderKind; 5] = [
    LoaderKind::Fabric,
    LoaderKind::LegacyFabric,
    LoaderKind::Quilt,
    LoaderKind::Forge,
    LoaderKind::NeoForge,
];

/// Fetches a loader's published versions for a single Minecraft version.
pub async fn versions(
    network: impl Network,
    kind: LoaderKind,
    game_version: &str,
) -> Result<Vec<LoaderVersion>, LoaderError> {
    let provider = match kind {
        LoaderKind::Fabric => LoaderApi::fabric(network),
        LoaderKind::LegacyFabric => LoaderApi::legacy_fabric(network),
        LoaderKind::Quilt => LoaderApi::quilt(network),
        LoaderKind::Forge => return ForgeApi::new(network).versions(game_version).await,
        LoaderKind::NeoForge => return NeoForgeApi::new(network).versions(game_version).await,
        loader => return Err(LoaderError::Unsupported { loader }),
    };
    provider.versions(game_version).await
}

/// Fabric's metadata service, whose scheme the `/versions/...` paths below are
/// written for.
const FABRIC_META: &str = "https://meta.fabricmc.net/v2";

/// Legacy Fabric's separate catalog for older Minecraft releases.
const LEGACY_FABRIC_META: &str = "https://meta.legacyfabric.net/v2";

/// Quilt's metadata service, whose scheme matches Fabric's.
const QUILT_META: &str = "https://meta.quiltmc.org/v3";

/// A loader whose metadata service publishes versions and version profiles.
///
/// The network provider is owned rather than borrowed so the type can be held
/// in the wiring for as long as the launcher runs, which is where a provider
/// is chosen once.
#[derive(Debug, Clone)]
pub struct LoaderApi<N: Network> {
    network: N,
    kind: LoaderKind,
    base: &'static str,
}

impl<N: Network> LoaderApi<N> {
    /// Fabric's metadata at its public service.
    pub fn fabric(network: N) -> Self {
        Self {
            network,
            kind: LoaderKind::Fabric,
            base: FABRIC_META,
        }
    }

    /// Legacy Fabric's Minecraft versions and launch profiles.
    pub fn legacy_fabric(network: N) -> Self {
        Self {
            network,
            kind: LoaderKind::LegacyFabric,
            base: LEGACY_FABRIC_META,
        }
    }

    /// Quilt's metadata at its public service.
    pub fn quilt(network: N) -> Self {
        Self {
            network,
            kind: LoaderKind::Quilt,
            base: QUILT_META,
        }
    }

    fn versions_url(&self, game_version: &str) -> String {
        format!("{}/versions/loader/{game_version}", self.base)
    }

    fn profile_url(&self, game_version: &str, loader_version: &str) -> String {
        format!(
            "{}/versions/loader/{game_version}/{loader_version}/profile/json",
            self.base
        )
    }

    fn parse_versions(
        &self,
        game_version: &str,
        response: Result<Vec<u8>, FetchError>,
    ) -> Result<Vec<LoaderVersion>, LoaderError> {
        let no_versions = || LoaderError::NoVersions {
            loader: self.kind,
            game_version: game_version.into(),
        };
        let bytes = match response {
            Ok(bytes) => bytes,
            // Fabric sends a 400 with an empty list for unsupported versions.
            Err(FetchError::Rejected { body, .. }) if is_empty_listing(&body) => {
                return Err(no_versions());
            }
            Err(source) => {
                return Err(LoaderError::Unavailable {
                    loader: self.kind,
                    source,
                });
            }
        };
        let entries: Vec<Entry> =
            serde_json::from_slice(&bytes).map_err(|error| LoaderError::Malformed {
                loader: self.kind,
                reason: format!("a listing that does not parse: {error}"),
            })?;
        if entries.is_empty() {
            return Err(no_versions());
        }
        Ok(entries
            .into_iter()
            .map(|entry| LoaderVersion {
                version: entry.loader.version,
                stable: entry.loader.stable,
            })
            .collect())
    }

    fn choose_version(
        &self,
        request: &LoaderRequest,
        response: Result<Vec<u8>, FetchError>,
    ) -> Result<String, LoaderError> {
        let offers = self.parse_versions(&request.game_version, response)?;
        let Some(requested) = &request.loader_version else {
            return Ok(recommended(&offers).version.clone());
        };
        offers
            .into_iter()
            .find(|offer| &offer.version == requested)
            .map(|offer| offer.version)
            .ok_or_else(|| LoaderError::UnknownVersion {
                loader: self.kind,
                version: requested.clone(),
            })
    }

    fn parse_profile(
        &self,
        game_version: &str,
        response: Result<Vec<u8>, FetchError>,
    ) -> Result<(ProfileHead, Vec<u8>), LoaderError> {
        let bytes = response.map_err(|source| LoaderError::Unavailable {
            loader: self.kind,
            source,
        })?;
        let head: ProfileHead =
            serde_json::from_slice(&bytes).map_err(|error| LoaderError::Malformed {
                loader: self.kind,
                reason: format!("a profile that does not parse: {error}"),
            })?;
        if head.id.trim().is_empty() {
            return Err(LoaderError::Malformed {
                loader: self.kind,
                reason: "a profile without an id".into(),
            });
        }
        if head.inherits_from.as_deref() != Some(game_version) {
            return Err(LoaderError::Malformed {
                loader: self.kind,
                reason: format!(
                    "a profile for {} that does not extend it",
                    head.inherits_from.as_deref().unwrap_or("nothing")
                ),
            });
        }
        Ok((head, bytes))
    }
}

/// One listing entry, as both services write it.
#[derive(Deserialize)]
struct Entry {
    loader: Tag,
}

#[derive(Deserialize)]
struct Tag {
    version: String,
    #[serde(default)]
    stable: Option<bool>,
}

/// The part of a version profile this provider must understand: its id, and
/// the vanilla version it extends.
#[derive(Deserialize)]
struct ProfileHead {
    id: String,
    #[serde(rename = "inheritsFrom")]
    inherits_from: Option<String>,
}

impl<N: Network> ModLoader for LoaderApi<N> {
    fn kind(&self) -> LoaderKind {
        self.kind
    }

    async fn versions(&self, game_version: &str) -> Result<Vec<LoaderVersion>, LoaderError> {
        let url = self.versions_url(game_version);
        self.network
            .fetch_with(&url, FetchKind::UnverifiedMetadata, |response| {
                self.parse_versions(game_version, response)
            })
            .await
    }

    async fn profile(&self, request: LoaderRequest) -> Result<LoaderProfile, LoaderError> {
        let listing = self.versions_url(&request.game_version);
        let chosen = self
            .network
            .fetch_with(&listing, FetchKind::UnverifiedMetadata, |response| {
                self.choose_version(&request, response)
            })
            .await?;

        let url = self.profile_url(&request.game_version, &chosen);
        let (head, bytes) = self
            .network
            .fetch_with(&url, FetchKind::UnverifiedMetadata, |response| {
                self.parse_profile(&request.game_version, response)
            })
            .await?;

        let (version_id, document) = if self.kind == LoaderKind::LegacyFabric {
            // The upstream id uses the same prefix as regular Fabric.
            (
                format!("legacy-{}", head.id),
                library::fill_native_classifiers(self.kind, bytes)?,
            )
        } else {
            (format!("{}-{chosen}", request.game_version), bytes)
        };
        Ok(LoaderProfile {
            version_id,
            document,
            files: Vec::new(),
        })
    }
}

/// The version to use when a caller does not name one: the newest stable entry
/// if there is one, otherwise the newest entry.
///
/// The services list newest first, so this is the first match in listing order
/// rather than a version comparison: a loader's scheme is its own, and parsing
/// it here would break on the first beta that spells itself differently.
fn recommended(offers: &[LoaderVersion]) -> &LoaderVersion {
    offers
        .iter()
        .find(|offer| offer.stable == Some(true))
        .unwrap_or(&offers[0])
}

/// Whether a rejected body is an empty JSON list.
fn is_empty_listing(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    value.as_array().is_some_and(|entries| entries.is_empty())
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/loader/src/lib.rs"]
mod tests;
