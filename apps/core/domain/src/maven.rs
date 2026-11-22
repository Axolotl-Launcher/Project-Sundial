// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Maven coordinates, the layout every Minecraft library is addressed by.
//!
//! A library is named `group:artifact:version[:classifier][@extension]`, and
//! its jar sits at `group/as/path/artifact/version/artifact-version
//! [-classifier].extension` under a repository root. The format is Mojang's,
//! the loaders' and the repositories' shared convention, so this lives in the
//! shared domain rather than in whichever system reads libraries first.

use std::path::PathBuf;

/// The file path a coordinate maps onto, relative to a repository root.
///
/// `None` when the coordinate has fewer than the three mandatory parts or any
/// of them is empty: there is no path to guess for `org.example` alone, and a
/// guessed path would address a different artifact.
pub fn path(coordinate: &str) -> Option<PathBuf> {
    let (body, extension) = match coordinate.split_once('@') {
        Some((body, extension)) => (body, extension),
        None => (coordinate, "jar"),
    };
    let mut parts = body.split(':');
    let group = parts.next().filter(|part| !part.is_empty())?;
    let artifact = parts.next().filter(|part| !part.is_empty())?;
    let version = parts.next().filter(|part| !part.is_empty())?;
    let classifier = parts.next().filter(|part| !part.is_empty());

    let file = match classifier {
        Some(classifier) => format!("{artifact}-{version}-{classifier}.{extension}"),
        None => format!("{artifact}-{version}.{extension}"),
    };
    let mut path = PathBuf::new();
    for segment in group.split('.') {
        path.push(segment);
    }
    Some(path.join(artifact).join(version).join(file))
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/domain/src/maven.rs"]
mod tests;
