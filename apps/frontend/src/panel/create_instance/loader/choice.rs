// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! What the creation form decides about a loader, independent of any widget.
//!
//! A loader version is never typed text: the form offers the builds its
//! provider publishes, one of which is the provider's own recommendation.
//! Choosing that one is spelled as a request without a version, so the
//! provider resolves it exactly as it would for an install asked the same way.

use axlauncher_core::spi::{LoaderError, LoaderKind, LoaderVersion};
use axlauncher_core::state::profile::Game;
use axlauncher_ui::{SearchableListItem, SharedString};

use super::providers;

/// What the form offers as a game: vanilla, or a loader this build serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoaderChoice {
    Vanilla,
    Loader(LoaderKind),
}

impl LoaderChoice {
    /// The provider this choice needs, or `None` for vanilla.
    pub(crate) fn loader(self) -> Option<LoaderKind> {
        match self {
            Self::Vanilla => None,
            Self::Loader(kind) => Some(kind),
        }
    }
}

impl SearchableListItem for LoaderChoice {
    type Value = Self;

    fn title(&self) -> SharedString {
        match self {
            Self::Vanilla => "Vanilla (no loader)".into(),
            Self::Loader(kind) => kind_label(*kind).into(),
        }
    }

    fn value(&self) -> &Self::Value {
        self
    }
}

/// Every choice the form lists, with vanilla first.
///
/// The loader entries come from the provider list, so a kind this build
/// cannot serve is never offered and a newly wired provider appears without
/// a second edit here.
pub(crate) fn choices() -> Vec<LoaderChoice> {
    std::iter::once(LoaderChoice::Vanilla)
        .chain(providers::KINDS.map(LoaderChoice::Loader))
        .collect()
}

/// The name a user reads for a loader kind; the wire spelling (`neoforge`) is
/// not what the picker should show.
pub(crate) fn kind_label(kind: LoaderKind) -> &'static str {
    match kind {
        LoaderKind::Fabric => "Fabric",
        LoaderKind::LegacyFabric => "Legacy Fabric",
        LoaderKind::Quilt => "Quilt",
        LoaderKind::Forge => "Forge",
        LoaderKind::NeoForge => "NeoForge",
        LoaderKind::OptiFine => "OptiFine",
    }
}

/// One row of the version dropdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VersionItem {
    version: String,
    recommended: bool,
}

impl SearchableListItem for VersionItem {
    type Value = String;

    fn title(&self) -> SharedString {
        if self.recommended {
            format!("{} (recommended)", self.version).into()
        } else {
            self.version.clone().into()
        }
    }

    fn value(&self) -> &Self::Value {
        &self.version
    }
}

/// The entry a request without a version resolves to.
///
/// Every provider picks the first entry it marks stable, and the first entry
/// otherwise. Reading that same rule here keeps the dropdown and the install
/// agreeing on which build the recommendation is.
pub(crate) fn recommended(versions: &[LoaderVersion]) -> Option<&LoaderVersion> {
    versions
        .iter()
        .find(|offer| offer.stable == Some(true))
        .or_else(|| versions.first())
}

/// The dropdown rows for one listing: the recommendation first, then the
/// rest in the order the provider listed them (newest first).
pub(crate) fn version_items(versions: &[LoaderVersion]) -> Vec<VersionItem> {
    let Some(pick) = recommended(versions) else {
        return Vec::new();
    };
    std::iter::once(VersionItem {
        version: pick.version.clone(),
        recommended: true,
    })
    .chain(
        versions
            .iter()
            .filter(|offer| offer.version != pick.version)
            .map(|offer| VersionItem {
                version: offer.version.clone(),
                recommended: false,
            }),
    )
    .collect()
}

/// The loader version an install request must name, or `None` when the
/// chosen entry is the recommendation: a request without a version asks the
/// provider for exactly that.
///
/// A listing with no recommendation names nothing, so there is no version an
/// install could ask for.
pub(crate) fn requested_version(
    versions: &[LoaderVersion],
    chosen: Option<&str>,
) -> Option<String> {
    let chosen = chosen?;
    let recommended = recommended(versions)?;
    (recommended.version != chosen).then(|| chosen.to_owned())
}

/// A game built from the form, or why it cannot be.
///
/// A loader needs a Minecraft version to list builds for and one of those
/// builds; vanilla needs only the version, and may omit it entirely.
pub(crate) fn game_for(
    kind: Option<LoaderKind>,
    minecraft: &str,
    versions: &[LoaderVersion],
    chosen: Option<&str>,
) -> Result<Option<Game>, String> {
    let minecraft = minecraft.trim();
    let Some(kind) = kind else {
        return Ok((!minecraft.is_empty()).then(|| Game::new(minecraft)));
    };
    if minecraft.is_empty() {
        return Err(format!(
            "Enter the Minecraft version to install {} for.",
            kind_label(kind)
        ));
    }
    if chosen.is_none() {
        return Err(format!(
            "Choose a {} version for Minecraft {minecraft}.",
            kind_label(kind)
        ));
    }
    let version = requested_version(versions, chosen);
    Ok(Some(
        Game::new(minecraft).with_loader(kind.as_str(), version),
    ))
}

/// Where the version list is in its lifecycle.
///
/// `Empty` and `Failed` carry the sentence the form shows, so a provider's
/// "no versions" answer reads as that and not as a generic failure.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LoadState {
    Idle,
    Loading,
    Ready,
    Empty(String),
    Failed(String),
}

impl LoadState {
    pub(crate) fn from_error(error: &LoaderError) -> Self {
        let loader = match error {
            LoaderError::Unavailable { loader, .. }
            | LoaderError::Malformed { loader, .. }
            | LoaderError::NoVersions { loader, .. }
            | LoaderError::UnknownVersion { loader, .. } => *loader,
        };
        match error {
            LoaderError::NoVersions { game_version, .. } => Self::Empty(format!(
                "{} has no builds for Minecraft {game_version}.",
                kind_label(loader)
            )),
            other => Self::Failed(format!(
                "Could not load {} versions: {other}",
                kind_label(loader)
            )),
        }
    }
}

#[cfg(test)]
#[path = "../../../../../../tests/apps/frontend/src/panel/create_instance/choice.rs"]
mod choice_tests;
