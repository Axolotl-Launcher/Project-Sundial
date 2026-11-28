// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io;
use std::path::Path;

use objc2_foundation::{
    NSError, NSFeatureUnsupportedError, NSFileManager, NSFileWriteUnsupportedSchemeError, NSString,
    NSURL,
};

pub(super) fn recycle(directory: &Path) -> io::Result<()> {
    // The trash crate formats macOS errors as strings, losing the error code.
    let url = NSURL::from_directory_path(directory).ok_or(io::ErrorKind::InvalidInput)?;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|error| {
            let kind = if recycle_unavailable(&error) {
                io::ErrorKind::Unsupported
            } else {
                io::ErrorKind::Other
            };
            io::Error::new(kind, error.to_string())
        })
}

fn recycle_unavailable(error: &NSError) -> bool {
    let domain = error.domain();
    let code = error.code();
    (domain == NSString::from_str("NSCocoaErrorDomain")
        && (code == NSFeatureUnsupportedError || code == NSFileWriteUnsupportedSchemeError))
        || (domain == NSString::from_str("NSPOSIXErrorDomain")
            && (code == libc::ENOTSUP as isize || code == libc::EOPNOTSUPP as isize))
        || error
            .underlyingErrors()
            .to_vec()
            .into_iter()
            .any(|error| recycle_unavailable(&error))
}
