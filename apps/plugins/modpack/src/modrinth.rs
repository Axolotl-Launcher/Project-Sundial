// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeMap, BTreeSet};

use axlauncher_spi::LoaderKind;
use serde::Deserialize;
use url::Url;

use crate::archive::{self, Archive, Entry};
use crate::{
    BundledFiles, Download, GameRequirements, ManifestLocation, OptionId, OptionalFile, Pack,
    PackError, PackFiles, PackReader, PackSource, PlannedDownloads, paths,
};

const MARKER: &str = "modrinth.index.json";

pub(crate) struct Reader;

impl PackReader for Reader {
    fn find_manifest(&self, archive: &Archive) -> Result<Option<ManifestLocation>, PackError> {
        Ok(archive.locate(MARKER))
    }

    fn read(&self, mut archive: Archive, manifest: ManifestLocation) -> Result<Pack, PackError> {
        let bytes = archive.manifest(manifest.entry, &manifest.path)?;
        let header: Format = decode(&bytes)?;
        if header.version != 1 || header.game != "minecraft" {
            return Err(PackError::Unsupported(format!(
                "Modrinth format {} for {}",
                header.version, header.game
            )));
        }
        let manifest: Manifest = decode(&bytes)?;
        manifest.into_pack(archive)
    }
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, PackError> {
    serde_json::from_slice(bytes).map_err(|error| PackError::Invalid(format!("{MARKER}: {error}")))
}

#[derive(Deserialize)]
struct Format {
    #[serde(rename = "formatVersion")]
    version: u64,
    game: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    name: String,
    version_id: String,
    dependencies: BTreeMap<String, String>,
    files: Vec<File>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    path: String,
    downloads: Vec<String>,
    file_size: u64,
    hashes: Hashes,
    env: Option<Environment>,
}

#[derive(Deserialize)]
struct Hashes {
    sha1: String,
    sha512: String,
}

#[derive(Deserialize)]
struct Environment {
    client: Support,
    #[serde(rename = "server")]
    _server: Support,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Support {
    #[default]
    Required,
    Optional,
    Unsupported,
}

impl Manifest {
    fn into_pack(self, archive: Archive) -> Result<Pack, PackError> {
        let name = self.name.trim().to_owned();
        if name.is_empty()
            || name.chars().any(char::is_control)
            || self.version_id.trim().is_empty()
        {
            return Err(PackError::Invalid(
                "pack name and version must contain visible text".into(),
            ));
        }
        let (minecraft, loader) = versions(self.dependencies)?;
        let overrides = overrides(&archive)?;
        let mut downloads = self
            .files
            .into_iter()
            .filter_map(File::download)
            .collect::<Result<Vec<_>, _>>()?;
        paths::check_tree(downloads.iter().map(|(file, _)| file.path()))?;
        downloads.retain(|(file, _)| !overrides.contains_key(file.path()));
        paths::check_tree(
            downloads
                .iter()
                .map(|(file, _)| file.path())
                .chain(overrides.keys().map(String::as_str)),
        )?;
        let optional = optional_files(&downloads);
        Ok(Pack {
            source: PackSource::Modrinth,
            name,
            version: self.version_id,
            game: GameRequirements { minecraft, loader },
            required_count: downloads.len() - optional.len(),
            optional,
            files: PackFiles::Modrinth(Files(downloads)),
            bundled: BundledFiles::new(archive::bundled(overrides)?, archive),
        })
    }
}

fn optional_files(downloads: &[(Download, bool)]) -> Vec<OptionalFile> {
    downloads
        .iter()
        .filter(|(_, optional)| *optional)
        .map(|(file, _)| OptionalFile {
            id: OptionId(file.path().to_owned()),
            label: file.path().to_owned(),
        })
        .collect()
}

pub(super) struct Files(Vec<(Download, bool)>);

impl Files {
    pub(super) fn plan(&self, selected: &BTreeSet<OptionId>) -> PlannedDownloads {
        let files = self
            .0
            .iter()
            .filter(|(file, optional)| !optional || selected.contains(file.path()))
            .map(|(file, _)| file.clone())
            .collect();
        PlannedDownloads {
            files,
            redirect_policy: |url| allows_target(url, true),
            cdn_key: None,
        }
    }
}

impl File {
    fn download(self) -> Option<Result<(Download, bool), PackError>> {
        let support = self
            .env
            .as_ref()
            .map_or(Support::Required, |env| env.client);
        if matches!(support, Support::Unsupported) {
            return None;
        }
        Some(
            self.into_download()
                .map(|file| (file, matches!(support, Support::Optional))),
        )
    }

    fn into_download(self) -> Result<Download, PackError> {
        paths::validate(&self.path)?;
        let mut sha512 = [0; 64];
        if self.hashes.sha1.len() != 40
            || !self
                .hashes
                .sha1
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || hex::decode_to_slice(&self.hashes.sha512, &mut sha512).is_err()
        {
            return Err(PackError::Invalid(format!(
                "invalid digest for {:?}",
                self.path
            )));
        }
        let urls: Vec<_> = self
            .downloads
            .iter()
            .filter(|raw| !raw.chars().any(char::is_whitespace))
            .filter_map(|raw| Url::parse(raw).ok().filter(|url| allows_target(url, false)))
            .map(String::from)
            .collect();
        if urls.is_empty() {
            return Err(PackError::Unsupported(format!(
                "no allowed HTTPS download source for {:?}",
                self.path
            )));
        }
        Ok(Download {
            path: self.path,
            urls,
            size_hint: Some(self.file_size),
            sha1: Some(self.hashes.sha1),
            sha512: Some(sha512),
            md5: None,
            manual_url: None,
        })
    }
}

fn allows_target(url: &Url, redirect: bool) -> bool {
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    match url.host_str() {
        Some("cdn.modrinth.com" | "github.com" | "raw.githubusercontent.com" | "gitlab.com") => {
            true
        }
        Some("release-assets.githubusercontent.com" | "objects.githubusercontent.com") => redirect,
        _ => false,
    }
}

fn versions(
    mut dependencies: BTreeMap<String, String>,
) -> Result<(String, Option<(LoaderKind, String)>), PackError> {
    let minecraft = dependencies
        .remove("minecraft")
        .ok_or_else(|| PackError::Invalid("missing Minecraft version".into()))?;
    paths::validate_version(&minecraft)?;
    if dependencies.len() > 1 {
        return Err(PackError::Unsupported(
            "the pack requires multiple game components besides Minecraft".into(),
        ));
    }
    let Some((kind, version)) = dependencies.into_iter().next() else {
        return Ok((minecraft, None));
    };
    let loader = match kind.as_str() {
        "fabric-loader" => LoaderKind::Fabric,
        "quilt-loader" => LoaderKind::Quilt,
        "forge" => LoaderKind::Forge,
        "neoforge" => LoaderKind::NeoForge,
        _ => {
            return Err(PackError::Unsupported(format!(
                "unknown dependency {kind:?}"
            )));
        }
    };
    paths::validate_version(&version)?;
    Ok((minecraft, Some((loader, version))))
}

fn overrides(archive: &Archive) -> Result<BTreeMap<String, Entry>, PackError> {
    let mut files = BTreeMap::new();
    for prefix in ["overrides/", "client-overrides/"] {
        files.extend(archive::read_layer(archive, prefix)?);
    }
    Ok(files)
}
