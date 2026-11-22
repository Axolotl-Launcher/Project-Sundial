// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Resolving a version that inherits from another.
//!
//! A loader's profile is a partial document: it names its vanilla version in
//! `inheritsFrom` and states only what it changes. The format defines how the
//! two combine, and the rules are not the same for every field:
//!
//! - a scalar the child states replaces the parent's, and one it omits is
//!   inherited;
//! - libraries and arguments are **appended**, child after parent, because the
//!   game is meant to see the loader's jars and flags after the vanilla ones;
//! - `jar` names the jar under `versions/` the game runs from, so a loader
//!   profile points at the vanilla client jar instead of one of its own.
//!
//! Resolving by field name, rather than by merging raw JSON, keeps a field the
//! game adds later visible instead of silently dropped.

use std::collections::HashSet;
use std::path::Path;

use crate::version::{self, Version};

/// How many links a chain may have before it is treated as a loop.
///
/// A real chain is two links, a nested loader at most three. The limit turns a
/// corrupt document into an error instead of a stack exhausted mid-launch.
const MAX_DEPTH: usize = 8;

/// Why a version could not be resolved against what it inherits from.
#[derive(Debug)]
pub enum ChainError {
    /// A document in the chain could not be read or parsed.
    Document { source: version::VersionError },
    /// A profile names the same parent twice, directly or through another.
    Cycle { id: String },
    /// The chain is longer than [`MAX_DEPTH`].
    TooDeep { id: String },
}

impl std::fmt::Display for ChainError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Document { source } => write!(formatter, "{source}"),
            Self::Cycle { id } => write!(formatter, "{id} inherits from itself"),
            Self::TooDeep { id } => write!(
                formatter,
                "{id} inherits through more than {MAX_DEPTH} documents"
            ),
        }
    }
}

impl std::error::Error for ChainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Document { source } => Some(source),
            Self::Cycle { .. } | Self::TooDeep { .. } => None,
        }
    }
}

/// The document `child` becomes once `parent` is beneath it.
///
/// The child's `id` is kept: it is the name the caller asked for, and the name
/// the game is told. `inherits_from` is cleared, so a resolved document no
/// longer claims to be a fragment of something else.
pub fn merge(parent: Version, child: Version) -> Version {
    let mut libraries = parent.libraries;
    libraries.extend(child.libraries);

    let arguments = match (parent.arguments, child.arguments) {
        (None, None) => None,
        (Some(only), None) | (None, Some(only)) => Some(only),
        (Some(mut parent), Some(child)) => {
            parent.jvm.extend(child.jvm);
            parent.game.extend(child.game);
            parent.default_user_jvm.extend(child.default_user_jvm);
            Some(parent)
        }
    };

    let mut data = parent.data;
    data.extend(child.data);
    let mut processors = parent.processors;
    processors.extend(child.processors);

    Version {
        id: child.id,
        kind: if child.kind.is_empty() {
            parent.kind
        } else {
            child.kind
        },
        main_class: if child.main_class.is_empty() {
            parent.main_class
        } else {
            child.main_class
        },
        libraries,
        arguments,
        minecraft_arguments: child.minecraft_arguments.or(parent.minecraft_arguments),
        asset_index: child.asset_index.or(parent.asset_index),
        assets: child.assets.or(parent.assets),
        java_version: child.java_version.or(parent.java_version),
        inherits_from: None,
        jar: child.jar.or(parent.jar),
        downloads: child.downloads.or(parent.downloads),
        logging: child.logging.or(parent.logging),
        processors,
        data,
    }
}

/// The document `version` describes once everything it inherits is beneath it.
pub fn resolve(root: &Path, version: Version) -> Result<Version, ChainError> {
    let mut visited = HashSet::new();
    resolve_below(root, version, &mut visited)
}

fn resolve_below(
    root: &Path,
    mut version: Version,
    visited: &mut HashSet<String>,
) -> Result<Version, ChainError> {
    if !visited.insert(version.id.clone()) {
        return Err(ChainError::Cycle { id: version.id });
    }
    if visited.len() > MAX_DEPTH {
        return Err(ChainError::TooDeep { id: version.id });
    }

    if let Some(parent_id) = version.inherits_from.clone() {
        let parent =
            version::load(root, &parent_id).map_err(|source| ChainError::Document { source })?;
        let parent = resolve_below(root, parent, visited)?;
        version = merge(parent, version);
    }

    visited.remove(&version.id);
    Ok(version)
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/chain.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/chain/document_tests.rs"]
mod document_tests;
