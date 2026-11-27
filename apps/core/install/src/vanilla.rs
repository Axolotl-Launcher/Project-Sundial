//! Official Minecraft catalog and local client validation.

use axlauncher_launch::{
    Target,
    version::{self, Version},
};
use axlauncher_storage::{GameDir, processor_marker::ProcessorMarker};

mod catalog;

pub use catalog::{MinecraftCatalog, MinecraftVersion, find_version, versions};

/// Vanilla readiness is a local check, independent of the network.
pub fn check_files(game_root: &GameDir, id: &str, target: &Target) -> Result<(), String> {
    check_id(id)?;
    if ProcessorMarker::for_version(game_root, id)
        .is_incomplete()
        .map_err(|error| error.to_string())?
    {
        return Err(format!("Installation processors for {id} did not finish."));
    }
    let version = version::load(game_root, id).map_err(|error| error.to_string())?;
    let version = vanilla_client(version, id)?;
    crate::prepare::check_local_files(game_root, &version, target)
}

pub fn check_id(id: &str) -> Result<(), String> {
    if !axlauncher_domain::is_version_id(id) {
        return Err(format!("Invalid Minecraft version ID {id:?}."));
    }
    Ok(())
}

/// Rejects manifests that are not the requested, self-contained vanilla client.
pub fn vanilla_client(version: Version, id: &str) -> Result<Version, String> {
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
