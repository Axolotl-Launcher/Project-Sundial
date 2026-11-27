// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Installer templates, read as the paths and literals they stand for.
//!
//! A processor's arguments and outputs are strings an installer wrote against
//! its own layout. `{NAME}` is a value from the profile's `data`, `[coord]` is a
//! jar already downloaded, and a quoted value is a literal. Some names are
//! neither: `{MINECRAFT_JAR}`, `{SIDE}` and their neighbours exist only because
//! the launcher chose the paths, so they are supplied here.
//!
//! Resolution fills every name from one place, which keeps a processor from
//! needing to know whether a value came from its profile or from the launcher.

use std::path::{Path, PathBuf};

use axlauncher_launch::version::Version;

use super::{CLIENT, ProcessorError};

/// The name of the side a processor runs for.
pub(super) const SIDE: &str = "SIDE";

/// The vanilla jar the loader patches.
pub(super) const MINECRAFT_JAR: &str = "MINECRAFT_JAR";

/// The game version the loader patches.
pub(super) const MINECRAFT_VERSION: &str = "MINECRAFT_VERSION";

/// The game directory used as this processor's working directory.
pub(super) const ROOT: &str = "ROOT";

/// The libraries directory the launcher chose.
pub(super) const LIBRARY_DIR: &str = "LIBRARY_DIR";

/// The values the launcher itself supplies, which no profile states.
pub(super) fn launcher_data(
    version: &Version,
    game_root: &Path,
    instance_directory: &Path,
    game_version: &str,
    libraries: &Path,
) -> Vec<(&'static str, String)> {
    let jar = axlauncher_storage::GameDir::standalone(game_root)
        .version_path(version.jar_id(), axlauncher_storage::VersionPath::ClientJar);
    vec![
        (SIDE, CLIENT.to_string()),
        (MINECRAFT_JAR, jar.display().to_string()),
        (MINECRAFT_VERSION, game_version.to_string()),
        (ROOT, instance_directory.display().to_string()),
        (LIBRARY_DIR, libraries.display().to_string()),
    ]
}

/// One argument or output template, with `{NAME}` and `[coord]` resolved.
///
/// A `[coord]` is the whole template; a `{NAME}` may sit inside surrounding
/// text, as `{ROOT}/libraries/` does, so the replacement is a substring search
/// rather than an exact match.
pub(super) fn resolve_template(
    template: &str,
    libraries: &Path,
    version: &Version,
    values: &[(&str, String)],
) -> Result<String, ProcessorError> {
    if let Some(coordinate) = bracket(template) {
        return library_path(libraries, coordinate)
            .ok_or_else(|| ProcessorError::ToolMissing {
                coordinate: coordinate.to_string(),
            })
            .map(|path| path.display().to_string());
    }

    let mut resolved = template.to_string();
    for (name, value) in values {
        resolved = resolved.replace(&format!("{{{name}}}"), value);
    }
    for (name, entry) in &version.data {
        let value = match bracket(&entry.client) {
            Some(coordinate) => library_path(libraries, coordinate)
                .ok_or_else(|| ProcessorError::ToolMissing {
                    coordinate: coordinate.to_string(),
                })?
                .display()
                .to_string(),
            None => unquote(&entry.client).to_string(),
        };
        resolved = resolved.replace(&format!("{{{name}}}"), &value);
    }
    Ok(resolved)
}

/// The text inside `[...]`, when a template is exactly that.
fn bracket(template: &str) -> Option<&str> {
    template
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
}

/// A `data` value written in single quotes is a literal, not a path.
fn unquote(value: &str) -> &str {
    value
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .unwrap_or(value)
}

/// Where a coordinate's jar lives under `libraries`.
pub(super) fn library_path(libraries: &Path, coordinate: &str) -> Option<PathBuf> {
    axlauncher_launch::version::maven_path(coordinate).map(|path| libraries.join(path))
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/install/src/processors/templates.rs"]
mod tests;
