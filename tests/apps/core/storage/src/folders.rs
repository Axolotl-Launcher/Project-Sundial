// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;

use super::*;

#[test]
fn an_unsupported_recycle_bin_allows_permanent_removal() {
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().join("instance");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("world"), b"save").unwrap();

    let result = finish_removal(&directory, Err(io::ErrorKind::Unsupported.into())).unwrap();

    assert_eq!(result, FolderRemoval::PermanentlyDeleted);
    assert!(!directory.exists());
}

#[test]
fn permission_errors_do_not_trigger_permanent_removal() {
    let fixture = tempfile::tempdir().unwrap();
    let result = finish_removal(fixture.path(), Err(io::ErrorKind::PermissionDenied.into()));

    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert!(fixture.path().is_dir());
}
