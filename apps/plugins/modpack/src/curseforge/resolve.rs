// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeMap, BTreeSet};

use axlauncher_spi::curseforge::{File, MINECRAFT_GAME_ID, Mod, ModpackCatalog};
use url::Url;

use super::{PinnedFile, allows_cdn, cdn_url};
use crate::{Download, PackError, paths};

pub(super) async fn downloads(
    catalog: &impl ModpackCatalog,
    pinned: &[&PinnedFile],
    automatic: bool,
) -> Result<Vec<Download>, PackError> {
    let file_ids: Vec<_> = pinned.iter().map(|file| file.file_id).collect();
    let project_ids: Vec<_> = pinned
        .iter()
        .map(|file| file.project_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let files: BTreeMap<_, _> = catalog
        .get_files(&file_ids)
        .await?
        .into_iter()
        .map(|file| (file.id, file))
        .collect();
    let projects: BTreeMap<_, _> = catalog
        .get_mods(&project_ids)
        .await?
        .into_iter()
        .map(|project| (project.id, project))
        .collect();
    pinned
        .iter()
        .map(|pin| resolve_file(pin, &files, &projects, automatic))
        .collect()
}

fn resolve_file(
    pin: &PinnedFile,
    files: &BTreeMap<u32, File>,
    projects: &BTreeMap<u32, Mod>,
    automatic: bool,
) -> Result<Download, PackError> {
    let file = files
        .get(&pin.file_id)
        .filter(|file| file.mod_id == pin.project_id && file.game_id == MINECRAFT_GAME_ID)
        .ok_or_else(|| {
            PackError::CurseForge(format!(
                "missing file {} from project {}",
                pin.file_id, pin.project_id
            ))
        })?;
    let project = projects
        .get(&pin.project_id)
        .filter(|project| project.game_id == MINECRAFT_GAME_ID)
        .ok_or_else(|| PackError::CurseForge(format!("missing project {}", pin.project_id)))?;
    download(file, project, automatic)
}

fn download(file: &File, project: &Mod, automatic: bool) -> Result<Download, PackError> {
    paths::validate(&file.file_name)?;
    if file.file_name.contains('/') {
        return Err(PackError::Invalid(format!(
            "invalid CurseForge file name {:?}",
            file.file_name
        )));
    }
    let sha1 = hash::<20>(file, 1)?.map(hex::encode);
    let md5 = hash::<16>(file, 2)?;
    let has_digest = sha1.is_some() || md5.is_some();
    let urls = urls(file, project, automatic, has_digest);
    let manual_url = has_digest.then(|| manual_page(project, file.id)).flatten();
    if urls.is_empty() && manual_url.is_none() {
        return Err(PackError::CurseForge(format!(
            "{} has no automatic source or verifiable manual download",
            file.file_name
        )));
    }
    Ok(Download {
        path: format!("{}/{}", folder(project)?, file.file_name),
        urls,
        size_hint: Some(file.file_length),
        sha1,
        sha512: None,
        md5,
        manual_url,
    })
}

fn urls(file: &File, project: &Mod, automatic: bool, has_digest: bool) -> Vec<String> {
    if !file.is_available || (!automatic && project.allow_mod_distribution == Some(false)) {
        return Vec::new();
    }
    let provided = file
        .download_url
        .as_deref()
        .and_then(|url| Url::parse(url.trim()).ok())
        .filter(allows_cdn);
    if let Some(provided) = provided {
        return vec![provided.into()];
    }
    if automatic && has_digest {
        return vec![cdn_url(file.id, &file.file_name)];
    }
    Vec::new()
}

fn hash<const BYTES: usize>(file: &File, algorithm: u32) -> Result<Option<[u8; BYTES]>, PackError> {
    let Some(hash) = file.hashes.iter().find(|hash| hash.algo == algorithm) else {
        return Ok(None);
    };
    let mut bytes = [0; BYTES];
    hex::decode_to_slice(&hash.value, &mut bytes)
        .map_err(|_| PackError::CurseForge(format!("invalid digest for {}", file.file_name)))?;
    Ok(Some(bytes))
}

fn folder(project: &Mod) -> Result<&'static str, PackError> {
    match project.class_id {
        Some(6) => Ok("mods"),
        Some(12) => Ok("resourcepacks"),
        Some(6552) => Ok("shaderpacks"),
        Some(6945) => Ok("datapacks"),
        class => Err(PackError::Unsupported(format!(
            "CurseForge project {} has unsupported content class {class:?}",
            project.id
        ))),
    }
}

fn manual_page(project: &Mod, file_id: u32) -> Option<String> {
    let mut url = project
        .links
        .website_url
        .as_deref()
        .and_then(|url| Url::parse(url).ok())
        .filter(|url| {
            url.scheme() == "https"
                && url.port().is_none()
                && url.username().is_empty()
                && url.password().is_none()
                && matches!(
                    url.host_str(),
                    Some("www.curseforge.com" | "curseforge.com")
                )
        })?;
    url.set_query(None);
    url.set_fragment(None);
    url.path_segments_mut()
        .expect("HTTPS URL permits paths")
        .pop_if_empty()
        .extend(["download", &file_id.to_string()]);
    Some(url.into())
}
