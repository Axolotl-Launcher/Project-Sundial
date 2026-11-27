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

use std::ffi::OsString;
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
) -> Vec<(&'static str, OsString)> {
    let jar = axlauncher_storage::GameDir::standalone(game_root)
        .version_path(version.jar_id(), axlauncher_storage::VersionPath::ClientJar);
    vec![
        (SIDE, CLIENT.into()),
        (MINECRAFT_JAR, jar.into_os_string()),
        (MINECRAFT_VERSION, game_version.into()),
        (ROOT, instance_directory.as_os_str().to_owned()),
        (LIBRARY_DIR, libraries.as_os_str().to_owned()),
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
    values: &[(&str, OsString)],
) -> Result<OsString, ProcessorError> {
    if let Some(coordinate) = bracket(template) {
        return coordinate_path(libraries, coordinate);
    }

    let mut resolved = OsString::new();
    let mut remaining = template;
    while let Some((prefix, parameter)) = remaining.split_once('{')
        && let Some((name, suffix)) = parameter.split_once('}')
    {
        resolved.push(prefix);
        if let Some((_, value)) = values.iter().find(|(key, _)| *key == name) {
            resolved.push(value);
        } else if let Some(value) = version.data.get(name) {
            resolved.push(match bracket(&value.client) {
                Some(coordinate) => coordinate_path(libraries, coordinate)?,
                None => unquote(&value.client).into(),
            });
        } else {
            let literal = OsString::from(format!("{{{name}}}"));
            resolved.push(literal);
        }
        remaining = suffix;
    }
    resolved.push(remaining);
    Ok(resolved)
}

fn coordinate_path(libraries: &Path, coordinate: &str) -> Result<OsString, ProcessorError> {
    library_path(libraries, coordinate)
        .map(PathBuf::into_os_string)
        .ok_or_else(|| ProcessorError::ToolMissing {
            coordinate: coordinate.to_owned(),
        })
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
#[path = "../../../../../../tests/apps/core/lib/installation/src/processors/templates.rs"]
mod tests;
