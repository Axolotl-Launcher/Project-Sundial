// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Folder publication and the namespaces shared with the profiles directory.

use std::io;
use std::path::{Path, PathBuf};

use axlauncher_storage_layout::{PROFILE_LOCK_PREFIX, STAGING_PREFIX};

/// Identifies names reserved for the launcher's lock and staging directories.
pub fn is_reserved_instance_directory(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with(PROFILE_LOCK_PREFIX) || name.starts_with(STAGING_PREFIX)
}

/// Allocates an unused sibling name for holding an instance during a move.
pub fn instance_move_path(directory: &Path) -> io::Result<PathBuf> {
    let parent = directory.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let reservation = tempfile::Builder::new()
        .prefix(STAGING_PREFIX)
        .tempdir_in(parent)?;
    let path = reservation.path().to_owned();
    reservation.close()?;
    Ok(path)
}

/// An existing destination stays intact, including an empty directory.
pub fn move_instance_folder(directory: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use rustix::fs::{CWD, RenameFlags, renameat_with};

        renameat_with(CWD, directory, CWD, destination, RenameFlags::NOREPLACE)?;
        Ok(())
    }
    #[cfg(windows)]
    {
        atomicwrites::move_atomic(&shell_path(directory)?, &shell_path(destination)?)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (directory, destination);
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(windows)]
fn shell_path(path: &Path) -> io::Result<PathBuf> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::ErrorKind::InvalidInput.into());
    };
    // Resolve the parent for Windows long paths, leaving the final entry
    // unresolved so a substituted junction can never move its target.
    Ok(parent.canonicalize()?.join(name))
}
