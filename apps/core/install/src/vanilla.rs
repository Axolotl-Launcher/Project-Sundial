// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Acquisition and preparation of official Minecraft client versions.

use std::fs::{self, File};
use std::io;
use std::path::Path;

use axlauncher_launch::Target;
use axlauncher_launch::version::{self, Version, VersionError};
use axlauncher_network::verify;
use axlauncher_spi::{FetchKind, Network};
use axlauncher_storage::write_atomic;

use crate::prepare::{Prepared, Progress, check_local_files, prepare_reporting};

mod catalog;

pub use catalog::{MinecraftCatalog, MinecraftVersion, MinecraftVersionKind, versions};

/// Installation uses the shared game root and reuses files already in place.
///
/// The lock covers the complete pass because different versions share libraries
/// and assets. The caller owns the profile and any UI progress state.
pub async fn install(
    network: &impl Network,
    game_root: &Path,
    id: &str,
    target: &Target,
    concurrency: usize,
    mut report: impl FnMut(Progress),
) -> Result<Prepared, String> {
    check_id(id)?;
    let _lock = installation_lock(game_root)?;
    report(Progress::Version);
    let version = ensure_version(network, game_root, id).await?;
    prepare_reporting(network, game_root, &version, target, concurrency, report)
        .await
        .map_err(|error| error.to_string())
}

/// A local installation check that works without a network connection.
pub fn check_files(game_root: &Path, id: &str, target: &Target) -> Result<(), String> {
    check_id(id)?;
    let version =
        version::load(&game_root.join("versions"), id).map_err(|error| error.to_string())?;
    let version = vanilla_client(version, id)?;
    check_local_files(game_root, &version, target)
}

fn check_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.ends_with('.')
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
    {
        return Err(format!("Invalid Minecraft version ID {id:?}."));
    }

    Ok(())
}

pub(crate) fn installation_lock(game_root: &Path) -> Result<File, String> {
    let lock = open_install_lock(game_root)?;
    lock.try_lock()
        .map_err(|error| format!("Cannot lock shared game files: {error}"))?;
    Ok(lock)
}

/// A shared guard keeps an installation stable while Minecraft starts.
pub fn launch_lock(game_root: &Path) -> Result<File, String> {
    let lock = open_install_lock(game_root)?;
    lock.try_lock_shared()
        .map_err(|error| format!("Cannot lock shared game files for launch: {error}"))?;
    Ok(lock)
}

fn open_install_lock(game_root: &Path) -> Result<File, String> {
    fs::create_dir_all(game_root)
        .map_err(|error| format!("Cannot create {}: {error}", game_root.display()))?;
    let path = game_root.join(".install.lock");
    File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| format!("Cannot open {}: {error}", path.display()))
}

pub(crate) async fn ensure_version(
    network: &impl Network,
    game_root: &Path,
    id: &str,
) -> Result<Version, String> {
    let version_root = game_root.join("versions");
    match version::load(&version_root, id) {
        Ok(version) => return vanilla_client(version, id),
        Err(VersionError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.to_string()),
    }

    let entry = catalog::find_version(network, id).await?;
    let (version, bytes) = network
        .fetch_with(&entry.url, FetchKind::Metadata, |response| {
            let bytes = response
                .map_err(|error| format!("Cannot fetch the Minecraft {id} manifest: {error}"))?;
            if !verify(&bytes, &entry.sha1) {
                return Err(format!(
                    "The Minecraft {id} manifest failed its SHA-1 check."
                ));
            }
            let version = serde_json::from_slice(&bytes)
                .map_err(|error| format!("Cannot read the Minecraft {id} manifest: {error}"))?;
            Ok((vanilla_client(version, id)?, bytes))
        })
        .await?;
    let path = crate::loader::document_path(game_root, id);
    write_atomic(&path, &bytes)
        .map_err(|error| format!("Cannot write {}: {error}", path.display()))?;
    Ok(version)
}

fn vanilla_client(version: Version, id: &str) -> Result<Version, String> {
    if version.id != id
        || version.inherits_from.is_some()
        || version.jar_id() != id
        || version
            .downloads
            .as_ref()
            .and_then(|downloads| downloads.client.as_ref())
            .is_none()
    {
        return Err(format!(
            "The local or downloaded manifest does not describe a Minecraft {id} client."
        ));
    }
    Ok(version)
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/install/src/vanilla.rs"]
mod tests;
