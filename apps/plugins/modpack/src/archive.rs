// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

use zip::ZipArchive;

use crate::{ManifestLocation, PackError, paths};

const MAX_ENTRIES: usize = 100_000;
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) struct Entry {
    index: usize,
    size: u64,
    regular: bool,
    directory: bool,
}

impl Entry {
    pub(crate) fn is_directory(self) -> bool {
        self.directory
    }

    pub(crate) fn size(self) -> u64 {
        self.size
    }

    pub(crate) fn require_file(self, path: &str) -> Result<(), PackError> {
        if self.regular {
            return Ok(());
        }
        Err(PackError::Invalid(format!("{path} must be a regular file")))
    }
}

pub(crate) struct Archive {
    zip: ZipArchive<BufReader<File>>,
    entries: BTreeMap<String, Entry>,
}

impl Archive {
    pub(crate) fn open(path: &Path) -> Result<Self, PackError> {
        let file = File::open(path)?;
        if !file.metadata()?.is_file() {
            return Err(PackError::Invalid("Select a local ZIP file".into()));
        }
        let mut zip = ZipArchive::new(BufReader::new(file))?;
        if zip.len() > MAX_ENTRIES || zip.has_duplicate_names() {
            return Err(PackError::Invalid(
                "ZIP has too many entries or duplicate names".into(),
            ));
        }
        let entries = read_entries(&mut zip)?;
        Ok(Self { zip, entries })
    }

    pub(crate) fn entry(&self, path: &str) -> Option<Entry> {
        self.entries.get(path).copied()
    }

    pub(super) fn locate(&self, path: &str) -> Option<ManifestLocation> {
        self.entry(path).map(|entry| ManifestLocation {
            path: path.to_owned(),
            entry,
        })
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = (&str, Entry)> {
        self.entries
            .iter()
            .map(|(path, entry)| (path.as_str(), *entry))
    }

    pub(crate) fn manifest(&mut self, entry: Entry, path: &str) -> Result<Vec<u8>, PackError> {
        entry.require_file(path)?;
        if entry.size > MAX_MANIFEST_BYTES {
            return Err(PackError::Invalid(format!("{path} is too large")));
        }
        let mut bytes = Vec::new();
        self.stream(entry, |input| input.read_to_end(&mut bytes).map(|_| ()))?;
        Ok(bytes)
    }

    pub(crate) fn stream(
        &mut self,
        entry: Entry,
        write: impl FnOnce(&mut dyn Read) -> io::Result<()>,
    ) -> Result<(), PackError> {
        let mut file = self.zip.by_index(entry.index)?.take(entry.size + 1);
        write(&mut file)?;
        let mut end = [0];
        if file.limit() != 1 || file.read(&mut end)? != 0 {
            return Err(PackError::Invalid(
                "ZIP member does not match its declared size".into(),
            ));
        }
        Ok(())
    }
}

fn read_entries(
    zip: &mut ZipArchive<BufReader<File>>,
) -> Result<BTreeMap<String, Entry>, PackError> {
    let mut entries = BTreeMap::new();
    for index in 0..zip.len() {
        let file = zip.by_index_raw(index)?;
        let name = file.name()?.into_owned();
        let directory = file.is_dir() && !file.is_symlink();
        let regular = file.is_file()
            && !file.is_symlink()
            && file
                .unix_mode()
                .is_none_or(|mode| matches!(mode & 0o170_000, 0 | 0o100_000));
        let entry = Entry {
            index,
            size: file.size(),
            regular,
            directory,
        };
        if entries.insert(name, entry).is_some() {
            return Err(PackError::Invalid(
                "ZIP contains duplicate decoded names".into(),
            ));
        }
    }
    Ok(entries)
}

pub(super) fn read_layer(
    archive: &Archive,
    prefix: &str,
) -> Result<BTreeMap<String, Entry>, PackError> {
    let mut layer = BTreeMap::new();
    for (name, entry) in archive.entries() {
        let Some(path) = name.strip_prefix(prefix).filter(|path| !path.is_empty()) else {
            continue;
        };
        if entry.is_directory() {
            continue;
        }
        entry.require_file(name)?;
        paths::validate(path)?;
        layer.insert(path.to_owned(), entry);
    }
    paths::check_tree(layer.keys().map(String::as_str))?;
    Ok(layer)
}

pub(super) fn bundled(entries: BTreeMap<String, Entry>) -> Result<Vec<(String, Entry)>, PackError> {
    const MAX_OVERRIDE_BYTES: u64 = 32 * 1024 * 1024 * 1024;
    let sizes = entries.values().copied().map(Entry::size);
    if sizes.fold(0_u64, u64::saturating_add) > MAX_OVERRIDE_BYTES {
        return Err(PackError::Invalid(
            "bundled files exceed the 32 GiB unpacked size limit".into(),
        ));
    }
    Ok(entries.into_iter().collect())
}
