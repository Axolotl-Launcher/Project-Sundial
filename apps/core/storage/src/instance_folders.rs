// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Folder publication and the namespaces shared with the profiles directory.

use std::io;
use std::path::Path;

use axlauncher_storage_layout::{PROFILE_LOCK_DIRECTORY, STAGING_PREFIX};

/// Identifies names reserved for the launcher's lock and staging directories.
pub fn is_reserved_instance_directory(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == PROFILE_LOCK_DIRECTORY || name.starts_with(STAGING_PREFIX)
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
        let (Some(parent), Some(name)) = (destination.parent(), destination.file_name()) else {
            return Err(io::ErrorKind::InvalidInput.into());
        };
        atomicwrites::move_atomic(
            &directory.canonicalize()?,
            &parent.canonicalize()?.join(name),
        )
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (directory, destination);
        Err(io::ErrorKind::Unsupported.into())
    }
}
