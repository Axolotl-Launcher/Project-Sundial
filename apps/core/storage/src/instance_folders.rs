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

/// Publishes the staged directory without replacing an existing entry.
pub fn publish_instance(staged: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use rustix::fs::{CWD, RenameFlags, renameat_with};

        renameat_with(CWD, staged, CWD, destination, RenameFlags::NOREPLACE)?;
        Ok(())
    }
    #[cfg(windows)]
    {
        // TempPath uses MoveFileEx without the replace flag. The directory
        // remains owned by its enclosing staging TempDir on failure.
        let mut staged = tempfile::TempPath::try_from_path(staged.canonicalize()?)?;
        staged.disable_cleanup(true);
        let (Some(parent), Some(name)) = (destination.parent(), destination.file_name()) else {
            return Err(io::ErrorKind::InvalidInput.into());
        };
        staged
            .persist_noclobber(parent.canonicalize()?.join(name))
            .map_err(|error| error.error)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (staged, destination);
        Err(io::ErrorKind::Unsupported.into())
    }
}
