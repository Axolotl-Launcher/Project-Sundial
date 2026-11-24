// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Maven coordinates, the layout every Minecraft library is addressed by.
//!
//! A library is named `group:artifact:version[:classifier][@extension]`, and
//! its jar sits at `group/as/path/artifact/version/artifact-version
//! [-classifier].extension` under a repository root. The format is Mojang's,
//! the loaders' and the repositories' shared convention, so this lives in the
//! shared domain rather than in whichever system reads libraries first.

use std::path::{Component, Path, PathBuf};

/// The file path a coordinate maps onto, relative to a repository root.
///
/// `None` when the coordinate has fewer than the three mandatory parts, any of
/// them is empty, or a part is not a single path component: there is no path to
/// guess for `org.example` alone, and a coordinate carrying `..` or an absolute
/// path would address a file outside the repository.
pub fn path(coordinate: &str) -> Option<PathBuf> {
    let (body, extension) = match coordinate.split_once('@') {
        Some((body, extension)) => (body, extension),
        None => (coordinate, "jar"),
    };
    if !safe_component(extension) {
        return None;
    }
    let mut parts = body.split(':');
    let group = parts.next().filter(|part| !part.is_empty())?;
    let artifact = parts.next().filter(|part| !part.is_empty())?;
    let version = parts.next().filter(|part| !part.is_empty())?;
    let classifier = parts.next().filter(|part| !part.is_empty());
    if parts.next().is_some() {
        return None;
    }
    let usable = group.split('.').all(safe_segment)
        && safe_component(artifact)
        && safe_component(version)
        && classifier.is_none_or(safe_component);
    if !usable {
        return None;
    }

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

/// Whether `value` is one directory name a path may hold.
fn safe_component(value: &str) -> bool {
    if value.is_empty() || value.contains(['/', '\\', ':']) || value.chars().any(char::is_control) {
        return false;
    }
    let mut components = Path::new(value).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

/// Whether `value` is one dotted group segment: a name, never a traversal.
fn safe_segment(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".." && safe_component(value)
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/domain/src/maven.rs"]
mod tests;
