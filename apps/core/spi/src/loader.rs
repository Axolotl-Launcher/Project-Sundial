// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Mod loaders: the port a Fabric, Quilt, Forge, NeoForge or OptiFine
//! provider implements.
//!
//! A launcher never runs a loader's own code to install it. It asks the
//! loader's metadata service for a version profile — a version document that
//! inherits from the vanilla game — and installs what that document names.
//! Every loader answers differently, so the contract stays at the level they
//! share:
//!
//! - which loader versions exist for a game version;
//! - the version document that turns a vanilla game into a loader-enabled one.
//!
//! Everything after the document is loader-independent and already lives
//! elsewhere: the install system writes it and downloads what it names, the
//! launch system resolves what it inherits and runs its processors. A provider
//! therefore needs no filesystem, no Java and no launcher state.

use std::fmt;
use std::future::Future;

use crate::FetchError;

/// Which mod loader a provider implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LoaderKind {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
    OptiFine,
}

impl LoaderKind {
    /// Every kind this build knows how to name, in the order a picker shows
    /// them.
    pub const ALL: [Self; 5] = [
        Self::Fabric,
        Self::Quilt,
        Self::Forge,
        Self::NeoForge,
        Self::OptiFine,
    ];

    /// The spelling this build stores in an instance and shows to a user.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fabric => "fabric",
            Self::Quilt => "quilt",
            Self::Forge => "forge",
            Self::NeoForge => "neoforge",
            Self::OptiFine => "optifine",
        }
    }

    /// The kind `value` names, ignoring case and surrounding space.
    ///
    /// A loader chosen elsewhere — a hand-edited manifest, an import — is read
    /// through this, so an unknown name is `None` rather than a wildcard.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

impl fmt::Display for LoaderKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One loader version a provider offers for a game version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderVersion {
    /// The loader's own version, as its metadata names it.
    pub version: String,
    /// Whether the provider marks it stable; `None` when it does not say.
    /// A provider's API is not obliged to have the concept.
    pub stable: Option<bool>,
}

/// What to install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderRequest {
    /// The vanilla version to attach the loader to.
    pub game_version: String,
    /// The loader version to install; `None` asks the provider to choose its
    /// recommended one.
    pub loader_version: Option<String>,
}

/// A loader resolved into what an install needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderProfile {
    /// The managed version id, which becomes a folder under `versions/`.
    pub version_id: String,
    /// The provider's version document, including any inheritance it declares.
    pub document: Vec<u8>,
    /// Files the profile refers to that no repository serves, at paths relative
    /// to the game root.
    ///
    /// Most loaders publish their libraries to a Maven repository and this is
    /// empty. A loader whose installer ships artifacts inside itself needs them
    /// on disk before its processors can find them; returning them here keeps
    /// the provider free of filesystem access and the write in one place.
    pub files: Vec<LoaderFile>,
}

/// One file a loader's installer places.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderFile {
    /// The path under the game root, with `/` separators.
    pub path: String,
    pub bytes: Vec<u8>,
}

/// Why a loader could not be listed or resolved.
#[derive(Debug)]
pub enum LoaderError {
    /// The metadata service could not be reached, or refused the request.
    Unavailable {
        loader: LoaderKind,
        source: FetchError,
    },
    /// The service answered with something that is not a loader listing or
    /// profile.
    Malformed { loader: LoaderKind, reason: String },
    /// The loader publishes no version for this game version.
    NoVersions {
        loader: LoaderKind,
        game_version: String,
    },
    /// The requested loader version is not one the provider offers.
    UnknownVersion { loader: LoaderKind, version: String },
}

impl fmt::Display for LoaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { loader, source } => {
                write!(formatter, "cannot reach {loader}'s metadata: {source}")
            }
            Self::Malformed { loader, reason } => {
                write!(formatter, "{loader} answered with {reason}")
            }
            Self::NoVersions {
                loader,
                game_version,
            } => write!(
                formatter,
                "{loader} has no version for Minecraft {game_version}"
            ),
            Self::UnknownVersion { loader, version } => {
                write!(formatter, "{loader} {version} is not offered")
            }
        }
    }
}

impl std::error::Error for LoaderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable { source, .. } => Some(source),
            Self::Malformed { .. } | Self::NoVersions { .. } | Self::UnknownVersion { .. } => None,
        }
    }
}

/// Loader versions and profiles from a loader's metadata service.
///
/// The two calls are separate because a picker lists versions long before it
/// resolves one, and a resolution must not fetch a listing a caller already
/// has.
pub trait ModLoader {
    /// Which loader this provider installs.
    fn kind(&self) -> LoaderKind;

    /// The loader versions offered for `game_version`, newest first.
    ///
    /// No version for that game version is [`LoaderError::NoVersions`], not an
    /// empty list: "this loader does not support 1.5.2" and "the request
    /// succeeded and there is nothing to show" lead a caller to different
    /// messages.
    fn versions(
        &self,
        game_version: &str,
    ) -> impl Future<Output = Result<Vec<LoaderVersion>, LoaderError>>;

    /// Resolves a loader into the profile an install writes and follows.
    fn profile(
        &self,
        request: LoaderRequest,
    ) -> impl Future<Output = Result<LoaderProfile, LoaderError>>;
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/spi/src/loader.rs"]
mod tests;
