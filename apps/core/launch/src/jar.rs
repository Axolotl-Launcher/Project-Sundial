// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Reading jar entries with the `zip` decoder.
//!
//! This is the production [`NativeReader`]: it opens a jar, walks its entries in
//! archive order, and hands their bytes to the extractor. A file that is not a
//! zip archive is reported as such rather than surfacing a decoder message, so a
//! damaged download reads differently from an unreadable archive.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::natives::{NativeError, NativeReader};

/// Reads entries from real jar files.
#[derive(Debug, Default, Clone, Copy)]
pub struct JarReader;

impl NativeReader for JarReader {
    fn read_jar(&self, path: &Path) -> Result<Vec<(String, Vec<u8>)>, NativeError> {
        let file = File::open(path).map_err(|source| NativeError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        let mut archive = zip::ZipArchive::new(file).map_err(|_| NativeError::NotAJar {
            path: path.to_path_buf(),
        })?;

        read_entries(&mut archive, path)
    }
}

fn read_entries<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    path: &Path,
) -> Result<Vec<(String, Vec<u8>)>, NativeError> {
    let mut entries = Vec::with_capacity(archive.len());

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| zip_error(path, &error))?;
        if file.is_dir() {
            continue;
        }
        let name = file
            .name()
            .map_err(|error| zip_error(path, &error))?
            .into_owned();

        let mut bytes = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
        file.read_to_end(&mut bytes)
            .map_err(|source| NativeError::Io {
                path: PathBuf::from(path),
                source,
            })?;
        entries.push((name, bytes));
    }

    Ok(entries)
}

fn zip_error(path: &Path, error: &zip::result::ZipError) -> NativeError {
    NativeError::Zip {
        path: path.to_path_buf(),
        reason: error.to_string(),
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/jar.rs"]
mod tests;
