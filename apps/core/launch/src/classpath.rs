// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The classpath a launch runs with.
//!
//! The separator follows the platform the game's Java runs on, not the host:
//! a Windows Java reads `;` and a Linux one `:`. The value is passed in rather
//! than read from a constant so a wrong pairing is visible at the call site.

use std::path::{Path, PathBuf};

use crate::rules::{self, Target};
use crate::version::Library;

/// The separator Windows Java expects.
pub const WINDOWS_SEPARATOR: char = ';';

/// The separator Java expects everywhere else.
pub const UNIX_SEPARATOR: char = ':';

/// The separator for a game running on `os`, in the game's vocabulary.
pub fn separator(os: &str) -> char {
    if os == "windows" {
        WINDOWS_SEPARATOR
    } else {
        UNIX_SEPARATOR
    }
}

/// The jar of a library, when the document names one for `target`.
///
/// The path is the one the document states, or the one the coordinate implies
/// when a loader names a Maven root instead.
///
/// A classifier jar carrying natives **is** a classpath entry. The 1.19 split
/// moved natives into ordinary four-part coordinates, and both things still
/// happen to them: the jar goes on the classpath and the files inside it are
/// extracted. LWJGL's loader looks for its shared library through the jar on
/// the classpath first, so leaving it out starts the game and fails inside it
/// with `Failed to locate library: lwjgl.dll`.
///
/// Only a declaration with no jar at all is excluded: the old shape lists
/// natives solely through `classifiers` and has no separate Java jar to offer.
pub fn artifact_path(library: &Library, target: &Target) -> Option<PathBuf> {
    if !rules::library_applies(library, target) || !library.include_in_classpath {
        return None;
    }
    if library.is_native_only() {
        return None;
    }
    library
        .download()
        .map(|artifact| PathBuf::from(artifact.path))
        .or_else(|| library.maven_path())
}

/// The jar a library contributes on `target`, resolved against `libraries_root`.
pub fn library_jar(library_root: &Path, library: &Library, target: &Target) -> Option<PathBuf> {
    artifact_path(library, target).map(|relative| library_root.join(relative))
}

/// The full classpath for a version, ending with its client jar.
///
/// Order matters to the game, so libraries keep document order and the client
/// jar comes last.
///
/// Two libraries can name the same artifact when a manifest carries
/// platform-specific revisions of one Maven module; only one jar may reach the
/// classpath or the older revision shadows the newer one's native ABI. The
/// later entry wins, so the list is walked from the end and the result is put
/// back in document order.
pub fn build(
    libraries_root: &Path,
    libraries: &[Library],
    client_jar: &Path,
    target: &Target,
) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut jars: Vec<String> = libraries
        .iter()
        .rev()
        .filter_map(|library| {
            // Resolve the jar first: a library that does not contribute one
            // must not claim the coordinate, or it would suppress the game
            // library an installer profile also lists as its own tool.
            let jar = library_jar(libraries_root, library, target)?;
            if !seen.insert(coordinate_key(library)) {
                return None;
            }
            Some(jar.display().to_string())
        })
        .collect();
    jars.reverse();
    jars.push(client_jar.display().to_string());

    jars.join(&separator(&target.os).to_string())
}

/// What makes two library entries the same artifact for classpath purposes.
///
/// The classifier is part of the key: a native classifier shares a group and
/// artifact with the Java jar but is a different file, and dropping it would
/// remove the Java classes.
fn coordinate_key(library: &Library) -> String {
    let mut parts = library.name.split(':');
    let group = parts.next().unwrap_or_default();
    let artifact = parts.next().unwrap_or_default();
    let classifier = parts.nth(1).unwrap_or_default();
    format!("{group}:{artifact}:{classifier}")
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/classpath.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/classpath_tool_shadow.rs"]
mod tool_shadow_tests;
