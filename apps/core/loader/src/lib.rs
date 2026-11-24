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
    FetchError, LoaderError, LoaderKind, LoaderProfile, LoaderRequest, LoaderVersion, ModLoader,
    Network,
};
use serde::Deserialize;

mod forge;
mod installer;
mod library;
mod neoforge;
mod versions;
pub use forge::ForgeApi;
pub use neoforge::NeoForgeApi;

/// Fabric's metadata service, whose scheme the `/versions/...` paths below are
/// written for.
const FABRIC_META: &str = "https://meta.fabricmc.net/v2";

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
        let bytes = match self.network.fetch(&url).await {
            Ok(bytes) => bytes,
            // Fabric answers a game version it has no builds for with a 400
            // whose body is an empty list. Without reading the body that is
            // indistinguishable from a broken request, which is the difference
            // between "this loader does not support 1.5.2" and "try again".
            Err(FetchError::Rejected { body, .. }) if is_empty_listing(&body) => {
                return Err(LoaderError::NoVersions {
                    loader: self.kind,
                    game_version: game_version.to_owned(),
                });
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

        let versions: Vec<LoaderVersion> = entries
            .into_iter()
            .map(|entry| LoaderVersion {
                version: entry.loader.version,
                stable: entry.loader.stable,
            })
            .collect();

        if versions.is_empty() {
            return Err(LoaderError::NoVersions {
                loader: self.kind,
                game_version: game_version.to_owned(),
            });
        }
        Ok(versions)
    }

    async fn profile(&self, request: LoaderRequest) -> Result<LoaderProfile, LoaderError> {
        let offers = self.versions(&request.game_version).await?;
        let chosen = match &request.loader_version {
            Some(requested) => offers
                .iter()
                .find(|offer| &offer.version == requested)
                .ok_or_else(|| LoaderError::UnknownVersion {
                    loader: self.kind,
                    version: requested.clone(),
                })?,
            None => recommended(&offers),
        };

        let url = self.profile_url(&request.game_version, &chosen.version);
        let bytes = self
            .network
            .fetch(&url)
            .await
            .map_err(|source| LoaderError::Unavailable {
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
        if head.inherits_from.as_deref() != Some(request.game_version.as_str()) {
            return Err(LoaderError::Malformed {
                loader: self.kind,
                reason: format!(
                    "a profile for {} that does not extend it",
                    head.inherits_from.as_deref().unwrap_or("nothing")
                ),
            });
        }

        Ok(LoaderProfile {
            version_id: format!("{}-{}", request.game_version, chosen.version),
            document: bytes,
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
