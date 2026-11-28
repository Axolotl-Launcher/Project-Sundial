// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop file manager and recycle bin integration.

use std::io;
use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;

/// How the directory was removed from its original location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRemoval {
    Recycled,
    PermanentlyDeleted,
}

/// Opens a directory with the desktop's file manager.
pub fn open_folder(directory: &Path) -> io::Result<()> {
    open::that(directory)
}

/// Moves a directory to the recycle bin, or removes it if recycling is unsupported.
pub fn remove_folder(directory: &Path) -> io::Result<FolderRemoval> {
    finish_removal(directory, recycle(directory))
}

fn finish_removal(directory: &Path, recycling: io::Result<()>) -> io::Result<FolderRemoval> {
    match recycling {
        Ok(()) => Ok(FolderRemoval::Recycled),
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {
            std::fs::remove_dir_all(directory)?;
            Ok(FolderRemoval::PermanentlyDeleted)
        }
        Err(error) => Err(error),
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn recycle(directory: &Path) -> io::Result<()> {
    trash::delete(directory).map_err(|error| {
        if recycle_unavailable(&error, directory) {
            io::Error::new(io::ErrorKind::Unsupported, error)
        } else {
            io::Error::other(error)
        }
    })
}

#[cfg(windows)]
fn recycle_unavailable(error: &trash::Error, _: &Path) -> bool {
    use windows_sys::Win32::UI::Shell::{
        COPYENGINE_E_RECYCLE_BIN_NOT_FOUND, COPYENGINE_E_RECYCLE_FORCE_NUKE,
    };

    matches!(
        error,
        trash::Error::Os {
            code: COPYENGINE_E_RECYCLE_BIN_NOT_FOUND | COPYENGINE_E_RECYCLE_FORCE_NUKE,
            ..
        }
    )
}

#[cfg(target_os = "linux")]
fn recycle_unavailable(error: &trash::Error, directory: &Path) -> bool {
    match error {
        trash::Error::FileSystem { path, source } => {
            !path.starts_with(directory)
                && matches!(
                    source.kind(),
                    io::ErrorKind::NotADirectory
                        | io::ErrorKind::ReadOnlyFilesystem
                        | io::ErrorKind::Unsupported
                )
        }
        // The Freedesktop backend has no separate error variant for a missing data home.
        trash::Error::Unknown { description } => {
            description == "Neither the XDG_DATA_HOME nor the HOME environment variable was found"
        }
        _ => false,
    }
}

#[cfg(target_os = "macos")]
use macos::recycle;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn recycle(_: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/storage/src/folders.rs"]
mod tests;
