//! Local readiness of a resolved mod loader installation.

use axlauncher_launch::{Target, chain};
use axlauncher_storage::{GameDir, processor_marker::ProcessorMarker};

/// Readiness uses the version profile written by the loader installation.
pub fn check_files(game_root: &GameDir, version_id: &str, target: &Target) -> Result<(), String> {
    let version = chain::load_resolved(game_root, version_id).map_err(|error| error.to_string())?;
    if ProcessorMarker::for_version(game_root, version_id)
        .is_incomplete()
        .map_err(|error| error.to_string())?
    {
        return Err(format!(
            "Installation processors for {version_id} did not finish."
        ));
    }
    crate::prepare::check_local_files(game_root, &version, target)
}
