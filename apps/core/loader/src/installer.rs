// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Reading a loader's installer archive.
//!
//! Forge and NeoForge ship an installer jar rather than a metadata service
//! answer. The jar holds what a launcher needs to turn it into a version
//! profile:
//!
//! - `version.json`: the profile itself, inheriting from the vanilla version;
//! - `install_profile.json`: the processors that patch the client jar, the
//!   tools they run, and the data entries their arguments refer to;
//! - `maven/`: those tools as files, for an installer that expects to find
//!   them beside itself;
//! - `data/`: inputs a data entry names by path inside the jar.
//!
//! The profile this module returns is the installer's own `version.json` with
//! the other three folded in, so one document describes everything an install
//! has to do. Path-valued data entries are rewritten to coordinates under an
//! `axlauncher.embedded` group whose files travel with the profile, which keeps
//! the document free of absolute paths: the caller decides where the game root
//! is, not the installer.

use std::io::{Cursor, Read};
use std::path::Path;

use axlauncher_domain::maven;
use axlauncher_spi::{LoaderError, LoaderFile, LoaderKind, LoaderProfile};
use serde_json::{Map, Value};

/// The installation document every modern installer carries.
const INSTALL_PROFILE: &str = "install_profile.json";

/// Where the version profile lives when the installer does not say.
const DEFAULT_PROFILE: &str = "/version.json";

/// The archive folder holding the installer's own Maven artifacts.
const EMBEDDED_MAVEN: &str = "maven/";

/// Where this build places a file it had to lift out of the installer.
const EMBEDDED_GROUP: &str = "axlauncher.embedded";

/// The side this launcher installs.
const CLIENT: &str = "client";

/// Reads `bytes` as an installer and returns the profile it describes.
pub(crate) fn parse(
    kind: LoaderKind,
    bytes: &[u8],
    game_version: &str,
) -> Result<LoaderProfile, LoaderError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| malformed(kind, format!("the installer is not a zip archive: {error}")))?;

    let install: Value = read_json(&mut archive, INSTALL_PROFILE, kind)?;
    // The oldest installers state their profile inline under `versionInfo`
    // instead of shipping it as `version.json`; naming that shape explains the
    // failure instead of reporting a missing archive entry.
    if install.get("versionInfo").is_some() {
        return Err(malformed(
            kind,
            "the installer uses the pre-1.9 layout, which this build cannot install",
        ));
    }
    let profile_path = install
        .get("json")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_PROFILE);
    let document: Value = read_json(&mut archive, profile_path.trim_start_matches('/'), kind)?;

    let mut object = document
        .as_object()
        .cloned()
        .ok_or_else(|| malformed(kind, "the installer's version profile is not an object"))?;
    let stated = object
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| install.get("version").and_then(Value::as_str))
        .ok_or_else(|| malformed(kind, "the installer names no version id"))?;
    // Forge and NeoForge's 1.20.1 line both call their profile
    // `1.20.1-forge-<build>`, so the installer's own id is not unique across
    // loaders. It becomes a directory that an install of the other loader would
    // overwrite; naming the loader keeps every provider's ids its own.
    let version_id = managed_id(kind, stated);

    let inherits = object
        .get("inheritsFrom")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if inherits != game_version {
        return Err(malformed(
            kind,
            format!("the installer extends {inherits:?}, not Minecraft {game_version}"),
        ));
    }

    // The document carries the id a launch asks for, so it names the same
    // managed directory the caller writes.
    object.insert("id".into(), Value::String(version_id.clone()));

    fold_processors(&mut object, &install);
    fold_libraries(&mut object, &install);
    let mut files = fold_data(kind, &mut archive, &mut object, &install, &version_id)?;
    files.extend(embedded_maven(kind, &mut archive)?);

    let document = serde_json::to_vec(&Value::Object(object))
        .map_err(|error| malformed(kind, error.to_string()))?;
    Ok(LoaderProfile {
        version_id,
        document,
        files,
    })
}

/// The version id this build manages for one installer's stated id.
///
/// NeoForge's 1.20.1 line still ships installers whose profile calls itself
/// `1.20.1-forge-<build>` — the same shape Forge publishes — so the id becomes a
/// directory that an install of the other loader overwrites. Naming the loader
/// keeps the two apart; every other id is kept as stated, so reinstalling a
/// build reaches the same directory.
fn managed_id(kind: LoaderKind, stated: &str) -> String {
    if kind == LoaderKind::NeoForge && stated.contains("-forge-") && !stated.contains("neoforge") {
        return stated.replace("-forge-", "-neoforge-");
    }
    stated.to_string()
}

/// Copies the installer's processors onto the profile.
fn fold_processors(object: &mut Map<String, Value>, install: &Value) {
    let processors = install.get("processors").cloned().unwrap_or(Value::Null);
    let empty = processors.as_array().is_none_or(Vec::is_empty);
    if !empty {
        object.insert("processors".into(), processors);
    }
}

/// Appends the installer's tools to the profile's libraries.
///
/// They are not game libraries: the processors run them, so they must be on
/// disk but never on the game's classpath. A library the profile already names
/// is left as the profile stated it.
fn fold_libraries(object: &mut Map<String, Value>, install: &Value) {
    let Some(tools) = install.get("libraries").and_then(Value::as_array) else {
        return;
    };
    let mut libraries = object
        .get("libraries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let named: Vec<String> = libraries
        .iter()
        .filter_map(|library| library.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect();

    for tool in tools {
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        if named.iter().any(|named| named == name) {
            continue;
        }
        let mut tool = tool.clone();
        if let Some(fields) = tool.as_object_mut() {
            fields.insert("include_in_classpath".into(), Value::Bool(false));
        }
        libraries.push(tool);
    }
    object.insert("libraries".into(), Value::Array(libraries));
}

/// Merges the installer's data entries, materialising path-shaped values.
fn fold_data(
    kind: LoaderKind,
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    object: &mut Map<String, Value>,
    install: &Value,
    version_id: &str,
) -> Result<Vec<LoaderFile>, LoaderError> {
    let Some(entries) = install.get("data").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    let mut data = object
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut files = Vec::new();

    for (name, entry) in entries {
        let mut entry = entry.as_object().cloned().unwrap_or_default();
        // Only the client side is materialised: this launcher installs and runs
        // the client, so a server input would be downloaded, written and never
        // used, and an installer that omits one would abort a client install.
        fold_side(
            kind, archive, &mut entry, name, CLIENT, version_id, &mut files,
        )?;
        data.insert(name.clone(), Value::Object(entry));
    }
    if !data.is_empty() {
        object.insert("data".into(), Value::Object(data));
    }
    Ok(files)
}

/// Rewrites one side of a data entry that names a file inside the installer.
fn fold_side(
    kind: LoaderKind,
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    entry: &mut Map<String, Value>,
    name: &str,
    side: &str,
    version_id: &str,
    files: &mut Vec<LoaderFile>,
) -> Result<(), LoaderError> {
    let Some(value) = entry.get(side).and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(inner) = value.strip_prefix('/') else {
        return Ok(());
    };
    let bytes = read_entry(archive, inner, kind)?;
    let coordinate = embedded_coordinate(version_id, name, side, inner);
    let relative = maven::path(&coordinate)
        .ok_or_else(|| malformed(kind, format!("{coordinate:?} has no Maven path")))?;
    files.push(LoaderFile {
        path: format!("libraries/{}", slash_path(&relative)),
        bytes,
    });
    entry.insert(side.into(), Value::String(format!("[{coordinate}]")));
    Ok(())
}

/// Lifts the installer's own Maven artifacts into the game's library tree.
fn embedded_maven(
    kind: LoaderKind,
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
) -> Result<Vec<LoaderFile>, LoaderError> {
    let names: Vec<String> = archive
        .file_names()
        .filter_map(Result::ok)
        .filter(|name| name.starts_with(EMBEDDED_MAVEN) && !name.ends_with('/'))
        .map(|name| name.into_owned())
        .collect();
    let mut files = Vec::with_capacity(names.len());
    for name in names {
        let relative = name.trim_start_matches(EMBEDDED_MAVEN);
        files.push(LoaderFile {
            path: format!("libraries/{relative}"),
            bytes: read_entry(archive, &name, kind)?,
        });
    }
    Ok(files)
}

/// The coordinate a lifted data file is addressed by.
///
/// The version id scopes the artifacts to one installer, and the side keeps a
/// client input and a server input from colliding when a data entry names both.
fn embedded_coordinate(version_id: &str, name: &str, side: &str, inner: &str) -> String {
    let extension = Path::new(inner)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("bin");
    let version = sanitise(version_id);
    let classifier = format!("{}-{side}", sanitise(&name.to_ascii_lowercase()));
    format!("{EMBEDDED_GROUP}:loader:{version}:{classifier}@{extension}")
}

/// Keeps a coordinate component to characters a Maven path can hold.
fn sanitise(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | '+') {
                character
            } else {
                '-'
            }
        })
        .collect()
}

/// A path as the document spells it, with `/` between segments.
fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn read_json(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
    kind: LoaderKind,
) -> Result<Value, LoaderError> {
    let bytes = read_entry(archive, name, kind)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| malformed(kind, format!("{name} does not parse: {error}")))
}

fn read_entry(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
    kind: LoaderKind,
) -> Result<Vec<u8>, LoaderError> {
    let mut entry = archive
        .by_name(name)
        .map_err(|_| malformed(kind, format!("the installer has no {name}")))?;
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| malformed(kind, format!("{name} cannot be read: {error}")))?;
    Ok(bytes)
}

fn malformed(kind: LoaderKind, reason: impl Into<String>) -> LoaderError {
    LoaderError::Malformed {
        loader: kind,
        reason: reason.into(),
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/loader/src/installer.rs"]
mod tests;
