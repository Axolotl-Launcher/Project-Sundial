// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeSet;
use std::sync::Mutex;

use axlauncher_spi::LoaderKind;
use axlauncher_spi::auth::AuthNetwork;
use serde::Deserialize;
use url::Url;

use crate::archive::{self, Archive, Entry};
use crate::{
    GameRequirements, OptionId, OptionalFile, Pack, PackError, PackFiles, PackReader, PackSource,
    PlanOptions, PlannedDownloads, paths,
};

mod api;

const MARKER: &str = "manifest.json";
const CDN: &str = "https://edge.forgecdn.net";

pub(super) struct Reader;

impl PackReader for Reader {
    fn find_manifest(&self, archive: &Archive) -> Option<Entry> {
        archive.entry(MARKER)
    }

    fn read(&self, mut archive: Archive, marker: Entry) -> Result<Pack, PackError> {
        let bytes = archive.manifest(marker, MARKER)?;
        let header: Header = decode(&bytes)?;
        if header.manifest_type != "minecraftModpack" || header.manifest_version != 1 {
            return Err(PackError::Unsupported(format!(
                "CurseForge manifest type {:?}, version {}",
                header.manifest_type, header.manifest_version
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
#[serde(rename_all = "camelCase")]
struct Header {
    manifest_type: String,
    manifest_version: u64,
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    #[serde(default)]
    version: String,
    minecraft: Minecraft,
    #[serde(default)]
    files: Vec<PinnedFile>,
    #[serde(default = "default_overrides")]
    overrides: String,
}

fn default_overrides() -> String {
    "overrides".into()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Minecraft {
    version: String,
    #[serde(default)]
    mod_loaders: Vec<Loader>,
}

#[derive(Deserialize)]
struct Loader {
    id: String,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize)]
struct PinnedFile {
    #[serde(rename = "projectID")]
    project_id: u32,
    #[serde(rename = "fileID")]
    file_id: u32,
    #[serde(default = "required_by_default")]
    required: bool,
}

fn required_by_default() -> bool {
    true
}

impl Manifest {
    fn into_pack(self, archive: Archive) -> Result<Pack, PackError> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(PackError::Invalid(
                "pack name must contain visible text".into(),
            ));
        }
        let bundled = if self.overrides.is_empty() {
            Vec::new()
        } else {
            paths::validate(&self.overrides)?;
            archive::bundled(archive::read_layer(
                &archive,
                &format!("{}/", self.overrides),
            )?)?
        };
        let optional = collect_optional(&self.files)?;
        Ok(Pack {
            source: PackSource::CurseForge,
            name: name.into(),
            version: self.version,
            game: self.minecraft.requirements()?,
            required_count: self.files.len() - optional.len(),
            optional,
            files: PackFiles::CurseForge(Files(self.files)),
            bundled,
            archive: Mutex::new(archive),
        })
    }
}

impl Minecraft {
    fn requirements(self) -> Result<GameRequirements, PackError> {
        paths::validate_version(&self.version)?;
        let mut primary = self.mod_loaders.iter().filter(|loader| loader.primary);
        let first = primary.next();
        if primary.next().is_some() || (first.is_none() && self.mod_loaders.len() > 1) {
            return Err(PackError::Unsupported(
                "ambiguous CurseForge mod loaders".into(),
            ));
        }
        let loader = first
            .or_else(|| self.mod_loaders.first())
            .map(|loader| loader.version(&self.version))
            .transpose()?;
        Ok(GameRequirements {
            minecraft: self.version,
            loader,
        })
    }
}

impl Loader {
    fn version(&self, minecraft: &str) -> Result<(LoaderKind, String), PackError> {
        let (kind, version) = self.id.split_once('-').ok_or_else(|| {
            PackError::Unsupported(format!("unknown CurseForge mod loader {:?}", self.id))
        })?;
        let kind = LoaderKind::parse(kind)
            .filter(|kind| {
                matches!(
                    kind,
                    LoaderKind::Forge
                        | LoaderKind::NeoForge
                        | LoaderKind::Fabric
                        | LoaderKind::Quilt
                )
            })
            .ok_or_else(|| {
                PackError::Unsupported(format!("unknown CurseForge mod loader {kind:?}"))
            })?;
        let version = match (kind, minecraft) {
            (LoaderKind::NeoForge, "1.20.1") => version.strip_prefix("1.20.1-").unwrap_or(version),
            _ => version,
        };
        paths::validate_version(version)?;
        Ok((kind, version.to_owned()))
    }
}

impl PinnedFile {
    fn selection_key(&self) -> String {
        format!("curseforge:{}:{}", self.project_id, self.file_id)
    }

    fn register(&self, seen: &mut BTreeSet<u32>) -> Result<(), PackError> {
        if self.project_id == 0 || self.file_id == 0 || !seen.insert(self.file_id) {
            return Err(PackError::Invalid(format!(
                "invalid or repeated CurseForge file ID {}",
                self.file_id
            )));
        }
        Ok(())
    }

    fn optional(&self) -> Option<OptionalFile> {
        (!self.required).then(|| OptionalFile {
            id: OptionId(self.selection_key()),
            label: format!(
                "CurseForge project {} · file {}",
                self.project_id, self.file_id
            ),
        })
    }
}

fn collect_optional(pinned: &[PinnedFile]) -> Result<Vec<OptionalFile>, PackError> {
    let mut ids = BTreeSet::new();
    let mut optional = Vec::new();
    for file in pinned {
        file.register(&mut ids)?;
        optional.extend(file.optional());
    }
    Ok(optional)
}

pub(super) struct Files(Vec<PinnedFile>);

impl Files {
    pub(super) async fn plan(
        &self,
        selected: &BTreeSet<OptionId>,
        options: PlanOptions,
        network: &impl AuthNetwork,
    ) -> Result<PlannedDownloads, PackError> {
        let pinned: Vec<_> = self
            .0
            .iter()
            .filter(|file| file.required || selected.contains(file.selection_key().as_str()))
            .collect();
        let mut planned = PlannedDownloads {
            files: Vec::new(),
            redirect_policy: allows_cdn,
            api_key: None,
        };
        if pinned.is_empty() {
            return Ok(planned);
        }
        let key = api_key().ok_or(PackError::MissingCurseForgeKey)?;
        planned.files = api::Client::new(network, &key)
            .resolve(&pinned, options.auto_download_restricted())
            .await?;
        planned.api_key = Some(key);
        Ok(planned)
    }
}

fn api_key() -> Option<String> {
    std::env::var("AXLAUNCHER_CURSEFORGE_API_KEY")
        .ok()
        .into_iter()
        .chain(option_env!("CURSEFORGE_API_KEY").map(str::to_owned))
        .find_map(|key| (!key.trim().is_empty()).then(|| key.trim().to_owned()))
}

fn allows_cdn(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some("edge.forgecdn.net" | "media.forgecdn.net")
        )
}

fn cdn_url(id: u32, file_name: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(file_name.as_bytes()).collect();
    format!(
        "{CDN}/files/{}/{}/{}",
        id / 1000,
        id % 1000,
        encoded.replace('+', "%20")
    )
}

#[cfg(test)]
#[path = "../../../../tests/apps/plugins/modpack/src/curseforge.rs"]
mod tests;
