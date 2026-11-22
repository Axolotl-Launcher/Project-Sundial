// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Local resource discovery by directory and file name.

use std::fs;
use std::io;
use std::path::Path;

use super::is_link;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Mod,
    ResourcePack,
    ShaderPack,
    DataPack,
    Schematic,
    World,
    Screenshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Resource {
    pub relative_path: String,
    pub kind: ResourceKind,
    pub size_bytes: Option<u64>,
    /// The suffix-derived state for content archives and schematics.
    pub enabled: Option<bool>,
}

/// The current local inventory, with paths relative to the game directory.
///
/// Only schematics recurse. World entries come from `level.dat` and screenshots
/// from PNG files. Results describe the disk even if the profile has no content
/// records. The scan neither opens archives nor hashes their contents.
pub fn scan_resources(root: &Path) -> io::Result<Vec<Resource>> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || is_link(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "invalid resource root",
        ));
    }
    let mut resources = Vec::new();
    for (folder, kind) in [
        ("mods", ResourceKind::Mod),
        ("resourcepacks", ResourceKind::ResourcePack),
        ("shaderpacks", ResourceKind::ShaderPack),
        ("datapacks", ResourceKind::DataPack),
        ("schematics", ResourceKind::Schematic),
        ("screenshots", ResourceKind::Screenshot),
    ] {
        scan_folder(root, folder, kind, &mut resources)?;
    }
    scan_worlds(root, &mut resources)?;
    resources.sort_unstable_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(resources)
}

fn scan_folder(
    root: &Path,
    folder: &str,
    kind: ResourceKind,
    resources: &mut Vec<Resource>,
) -> io::Result<()> {
    let mut pending = vec![folder.to_owned()];
    while let Some(folder) = pending.pop() {
        visit(&root.join(&folder), |entry, metadata| {
            collect_entry(&folder, kind, &entry, &metadata, &mut pending, resources);
            Ok(())
        })?;
    }
    Ok(())
}

fn collect_entry(
    folder: &str,
    kind: ResourceKind,
    entry: &fs::DirEntry,
    metadata: &fs::Metadata,
    pending: &mut Vec<String>,
    resources: &mut Vec<Resource>,
) {
    let Ok(name) = entry.file_name().into_string() else {
        return;
    };
    let relative_path = format!("{folder}/{name}");
    if metadata.is_dir() && kind == ResourceKind::Schematic {
        pending.push(relative_path);
    } else if metadata.is_file()
        && let Some(enabled) = resource_state(kind, &name)
    {
        resources.push(Resource {
            relative_path,
            kind,
            size_bytes: Some(metadata.len()),
            enabled,
        });
    }
}

fn scan_worlds(root: &Path, resources: &mut Vec<Resource>) -> io::Result<()> {
    visit(&root.join("saves"), |entry, metadata| {
        let Some(name) = entry
            .file_name()
            .into_string()
            .ok()
            .filter(|_| metadata.is_dir())
        else {
            return Ok(());
        };
        let relative_path = format!("saves/{name}");
        let level = root.join(&relative_path).join("level.dat");
        if fs::symlink_metadata(level)
            .is_ok_and(|metadata| metadata.is_file() && !is_link(&metadata))
        {
            resources.push(Resource {
                relative_path: relative_path.clone(),
                kind: ResourceKind::World,
                size_bytes: None,
                enabled: None,
            });
        }
        scan_folder(
            root,
            &format!("{relative_path}/datapacks"),
            ResourceKind::DataPack,
            resources,
        )
    })
}

fn visit(
    directory: &Path,
    mut entry: impl FnMut(fs::DirEntry, fs::Metadata) -> io::Result<()>,
) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.is_dir() || is_link(&metadata) {
        return Ok(());
    }
    for item in fs::read_dir(directory)? {
        let item = item?;
        match item.metadata() {
            Ok(metadata) if !is_link(&metadata) => entry(item, metadata)?,
            Ok(_) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn resource_state(kind: ResourceKind, name: &str) -> Option<Option<bool>> {
    if kind == ResourceKind::Screenshot {
        return has_extension(name, &["png"]).then_some(None);
    }
    let enabled = !name.ends_with(".disabled");
    let name = name.trim_end_matches(".disabled");
    let extensions: &[&str] = match kind {
        ResourceKind::Mod => &["jar"],
        ResourceKind::ResourcePack | ResourceKind::ShaderPack | ResourceKind::DataPack => &["zip"],
        ResourceKind::Schematic => &["litematic", "schematic", "schem", "nbt"],
        ResourceKind::World | ResourceKind::Screenshot => return None,
    };
    has_extension(name, extensions).then_some(Some(enabled))
}

fn has_extension(name: &str, extensions: &[&str]) -> bool {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            extensions
                .iter()
                .any(|expected| ext.eq_ignore_ascii_case(expected))
        })
}
