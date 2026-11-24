// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::Path;
use std::{fs, io};

/// A unique console log that is removed on drop until the child starts.
pub fn open_instance_console_log(instance: &Path) -> io::Result<tempfile::NamedTempFile> {
    let instance = instance.canonicalize()?;
    let logs = instance.join("logs");
    match fs::create_dir(&logs) {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error),
    }
    let metadata = logs.symlink_metadata()?;
    if !metadata.is_dir() || is_redirect(&metadata) || logs.canonicalize()? != logs {
        return Err(io::Error::other("instance log directory is redirected"));
    }
    tempfile::Builder::new()
        .prefix("launcher-console-")
        .suffix(".log")
        .tempfile_in(logs)
}

#[cfg(windows)]
fn is_redirect(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_redirect(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}
