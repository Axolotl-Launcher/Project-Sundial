// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeMap, BTreeSet};

use axlauncher_spi::LoaderKind;
use serde::Deserialize;

use crate::archive::{self, Archive, Entry};
use crate::{
    BundledFiles, GameRequirements, ManifestLocation, Pack, PackError, PackFiles, PackReader,
    PackSource, paths,
};

const COMPONENTS: &str = "mmc-pack.json";
const CONFIG: &str = "instance.cfg";
const MINECRAFT: &str = "net.minecraft";
const INTERMEDIARY: &str = "net.fabricmc.intermediary";

pub(super) struct Reader;

impl PackReader for Reader {
    fn find_manifest(&self, archive: &Archive) -> Result<Option<ManifestLocation>, PackError> {
        let mut candidates = archive.entries().filter_map(|(path, entry)| {
            let root = instance_root(path)?;
            archive.entry(&format!("{root}{CONFIG}"))?;
            Some(ManifestLocation {
                path: path.to_owned(),
                entry,
            })
        });
        let found = candidates.next();
        if candidates.next().is_some() {
            return Err(PackError::AmbiguousFormat);
        }
        Ok(found)
    }

    fn read(&self, mut archive: Archive, manifest: ManifestLocation) -> Result<Pack, PackError> {
        let root = instance_root(&manifest.path).expect("MultiMC reader selected the manifest");
        let wrapper = root.trim_end_matches('/');
        if !root.is_empty() {
            paths::validate(wrapper)?;
        }
        let bytes = archive.manifest(manifest.entry, &manifest.path)?;
        let Header { format_version } = decode(&bytes)?;
        if format_version != 1 {
            return Err(PackError::Unsupported(format!(
                "MultiMC component format {format_version}"
            )));
        }
        let Manifest { components } = decode(&bytes)?;
        let game = requirements(&components)?;
        validate_local_artifacts(&archive, root, &components)?;
        let bundled = archive::bundled(game_files(&archive, root)?)?;
        let name = read_instance_name(&mut archive, root, wrapper)?;
        Ok(Pack {
            source: PackSource::MultiMc,
            name,
            version: String::new(),
            game,
            required_count: 0,
            optional: Vec::new(),
            files: PackFiles::Bundled,
            bundled: BundledFiles::new(bundled, archive),
        })
    }
}

fn instance_root(path: &str) -> Option<&str> {
    let root = path.strip_suffix(COMPONENTS)?;
    if root.is_empty() {
        return Some(root);
    }
    let folder = root.strip_suffix('/')?;
    (!folder.is_empty() && !folder.contains('/')).then_some(root)
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, PackError> {
    serde_json::from_slice(bytes)
        .map_err(|error| PackError::Invalid(format!("{COMPONENTS}: {error}")))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    format_version: u64,
}

#[derive(Deserialize)]
struct Manifest {
    components: Vec<Component>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Component {
    uid: String,
    version: Option<String>,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    dependency_only: bool,
}

impl Component {
    fn version(&self) -> Result<&str, PackError> {
        let version = self.version.as_deref().ok_or_else(|| {
            PackError::Unsupported(format!(
                "MultiMC component {} has no selected version",
                self.uid
            ))
        })?;
        paths::validate_version(version)?;
        Ok(version)
    }
}

fn requirements(components: &[Component]) -> Result<GameRequirements, PackError> {
    let mut active = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for component in components {
        register_component(component, &mut seen, &mut active)?;
    }
    let minecraft = active
        .remove(MINECRAFT)
        .ok_or_else(|| {
            PackError::Invalid("MultiMC pack has no enabled Minecraft component".into())
        })?
        .version()?;
    let mappings = active.remove(INTERMEDIARY);
    let mut loader = None;
    for component in active.into_values() {
        if let Some(next) = loader_component(component)? {
            combine_loader(&mut loader, next)?;
        }
    }
    validate_mappings(mappings, loader.as_ref().map(|(kind, _)| *kind), minecraft)?;
    Ok(GameRequirements {
        minecraft: minecraft.to_owned(),
        loader,
    })
}

fn register_component<'a>(
    component: &'a Component,
    seen: &mut BTreeSet<String>,
    active: &mut BTreeMap<&'a str, &'a Component>,
) -> Result<(), PackError> {
    let uid = component.uid.as_str();
    if uid.is_empty()
        || !uid
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        || !seen.insert(uid.to_ascii_lowercase())
    {
        return Err(PackError::Invalid(format!(
            "invalid or repeated MultiMC component {uid:?}"
        )));
    }
    if !component.disabled {
        active.insert(uid, component);
    }
    Ok(())
}

fn loader_component(component: &Component) -> Result<Option<(LoaderKind, String)>, PackError> {
    let kind = match component.uid.as_str() {
        "net.minecraftforge" => LoaderKind::Forge,
        "net.neoforged" => LoaderKind::NeoForge,
        "net.fabricmc.fabric-loader" => LoaderKind::Fabric,
        "org.quiltmc.quilt-loader" => LoaderKind::Quilt,
        "org.lwjgl" | "org.lwjgl3" if component.dependency_only || component.version.is_none() => {
            return Ok(None);
        }
        other => {
            return Err(PackError::Unsupported(format!(
                "MultiMC component {other} cannot be mapped to a standard game installation"
            )));
        }
    };
    Ok(Some((kind, component.version()?.to_owned())))
}

fn combine_loader(
    loader: &mut Option<(LoaderKind, String)>,
    next: (LoaderKind, String),
) -> Result<(), PackError> {
    if loader.replace(next).is_some() {
        return Err(PackError::Unsupported(
            "MultiMC pack enables more than one mod loader".into(),
        ));
    }
    Ok(())
}

fn validate_mappings(
    mappings: Option<&Component>,
    loader: Option<LoaderKind>,
    minecraft: &str,
) -> Result<(), PackError> {
    if let Some(mappings) = mappings
        && (!matches!(loader, Some(LoaderKind::Fabric | LoaderKind::Quilt))
            || mappings
                .version
                .as_deref()
                .is_some_and(|version| version != minecraft))
    {
        return Err(PackError::Unsupported(
            "MultiMC pack requires nonstandard Fabric intermediary mappings".into(),
        ));
    }
    Ok(())
}

fn validate_local_artifacts(
    archive: &Archive,
    root: &str,
    components: &[Component],
) -> Result<(), PackError> {
    let active_patches: BTreeSet<_> = components
        .iter()
        .filter(|component| !component.disabled)
        .map(|component| format!("patches/{}.json", component.uid.to_ascii_lowercase()))
        .collect();
    let payload = archive
        .entries()
        .filter(|(_, entry)| !entry.is_directory())
        .filter_map(|(path, _)| path.strip_prefix(root));
    for path in payload {
        let lower = path.to_ascii_lowercase();
        let directory = lower.split('/').next().unwrap_or_default();
        if matches!(directory, "libraries" | "jarmods") || active_patches.contains(&lower) {
            return Err(PackError::Unsupported(format!(
                "MultiMC pack contains a local launch artifact: {path}"
            )));
        }
    }
    Ok(())
}

fn game_files(archive: &Archive, root: &str) -> Result<BTreeMap<String, Entry>, PackError> {
    for directory in [".minecraft", "minecraft"] {
        let path = format!("{root}{directory}");
        if let Some(files) = read_game_directory(archive, &path)? {
            return Ok(files);
        }
    }
    Ok(BTreeMap::new())
}

fn read_game_directory(
    archive: &Archive,
    path: &str,
) -> Result<Option<BTreeMap<String, Entry>>, PackError> {
    let prefix = format!("{path}/");
    if archive.entry(path).is_some()
        || archive
            .entry(&prefix)
            .is_some_and(|entry| !entry.is_directory())
    {
        return Err(PackError::Invalid(format!("{path} must be a directory")));
    }
    let files = archive::read_layer(archive, &prefix)?;
    let exists = !files.is_empty()
        || archive
            .entries()
            .any(|(entry, _)| entry.starts_with(&prefix));
    Ok(exists.then_some(files))
}

fn read_instance_name(
    archive: &mut Archive,
    root: &str,
    wrapper: &str,
) -> Result<String, PackError> {
    let path = format!("{root}{CONFIG}");
    let entry = archive
        .entry(&path)
        .expect("MultiMC detection requires instance.cfg");
    let bytes = archive.manifest(entry, &path)?;
    let config = std::str::from_utf8(&bytes)
        .map_err(|_| PackError::Invalid(format!("{path} is not UTF-8")))?;
    let name = config
        .trim_start_matches('\u{feff}')
        .lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| key.trim() == "name")
        .map(|(_, value)| display_name(value))
        .next_back();
    let fallback = if wrapper.is_empty() {
        "MultiMC instance"
    } else {
        wrapper
    };
    Ok(name
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| fallback.to_owned()))
}

fn display_name(value: &str) -> String {
    let mut name = String::new();
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        let next = match character {
            '#' => break,
            '\\' => chars.next().map(|escaped| match escaped {
                'n' | 't' => ' ',
                character => character,
            }),
            character => Some(character),
        };
        name.extend(next.map(|character| match character {
            control if control.is_control() => ' ',
            visible => visible,
        }));
    }
    name.trim().to_owned()
}
