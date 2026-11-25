// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeMap, BTreeSet};

use axlauncher_spi::auth::{AuthNetwork, Body, Request};
use serde::Deserialize;
use url::Url;

use super::{PinnedFile, allows_cdn, cdn_url};
use crate::{Download, PackError, paths};

const GAME_ID: u32 = 432;
const API_ROOT: &str = "https://api.curseforge.com/v1";
const USER_AGENT: &str = concat!("Axolotl-Launcher/", env!("CARGO_PKG_VERSION"));

pub(super) struct Client<'a, N> {
    network: &'a N,
    key: &'a str,
}

impl<'a, N: AuthNetwork> Client<'a, N> {
    pub(super) fn new(network: &'a N, key: &'a str) -> Self {
        Self { network, key }
    }

    pub(super) async fn resolve(
        &self,
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
        let files: BTreeMap<_, _> = self
            .batch::<RemoteFile>("/mods/files", "fileIds", &file_ids, 25)
            .await?
            .into_iter()
            .map(|file| (file.id, file))
            .collect();
        let projects: BTreeMap<_, _> = self
            .batch::<Project>("/mods", "modIds", &project_ids, 10)
            .await?
            .into_iter()
            .map(|project| (project.id, project))
            .collect();
        pinned
            .iter()
            .map(|pin| resolve_file(pin, &files, &projects, automatic))
            .collect()
    }

    async fn batch<T: serde::de::DeserializeOwned>(
        &self,
        route: &str,
        field: &str,
        ids: &[u32],
        limit: usize,
    ) -> Result<Vec<T>, PackError> {
        let url = format!("{API_ROOT}{route}");
        let mut results = Vec::new();
        for chunk in ids.chunks(limit) {
            let body = serde_json::json!({ field: chunk }).to_string();
            let request = Request::new(&url, USER_AGENT)
                .with_api_key(self.key)
                .with_body(Body::Json(body));
            let response = self
                .network
                .send(request)
                .await
                .map_err(|_| PackError::CurseForge(format!("request to {route} failed")))?;
            results.extend(decode::<T>(response, route)?);
        }
        Ok(results)
    }
}

fn resolve_file(
    pin: &PinnedFile,
    files: &BTreeMap<u32, RemoteFile>,
    projects: &BTreeMap<u32, Project>,
    automatic: bool,
) -> Result<Download, PackError> {
    let file = files
        .get(&pin.file_id)
        .filter(|file| file.mod_id == pin.project_id && file.game_id == GAME_ID)
        .ok_or_else(|| {
            PackError::CurseForge(format!(
                "missing file {} from project {}",
                pin.file_id, pin.project_id
            ))
        })?;
    let project = projects
        .get(&pin.project_id)
        .filter(|project| project.game_id == GAME_ID)
        .ok_or_else(|| PackError::CurseForge(format!("missing project {}", pin.project_id)))?;
    file.download(project, automatic)
}

fn decode<T: serde::de::DeserializeOwned>(
    response: axlauncher_spi::auth::Response,
    route: &str,
) -> Result<Vec<T>, PackError> {
    if response.status != 200 {
        let reason = match response.status {
            401 | 403 => "the API key was rejected",
            429 => "the API rate limit was reached",
            _ => "unexpected API response",
        };
        return Err(PackError::CurseForge(format!(
            "{route}: {reason} (HTTP {})",
            response.status
        )));
    }
    let response: Envelope<T> = serde_json::from_slice(&response.body).map_err(|error| {
        PackError::CurseForge(format!("invalid response from {route}: {error}"))
    })?;
    Ok(response.data)
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteFile {
    id: u32,
    mod_id: u32,
    game_id: u32,
    file_name: String,
    file_length: u64,
    download_url: Option<String>,
    is_available: bool,
    #[serde(default)]
    hashes: Vec<FileHash>,
}

#[derive(Deserialize)]
struct FileHash {
    algo: u32,
    value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    id: u32,
    game_id: u32,
    class_id: Option<u32>,
    allow_mod_distribution: Option<bool>,
    links: Links,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Links {
    website_url: Option<String>,
}

impl RemoteFile {
    fn download(&self, project: &Project, automatic: bool) -> Result<Download, PackError> {
        paths::validate(&self.file_name)?;
        if self.file_name.contains('/') {
            return Err(PackError::Invalid(format!(
                "invalid CurseForge file name {:?}",
                self.file_name
            )));
        }
        let sha1 = self.hash::<20>(1)?.map(hex::encode);
        let md5 = self.hash::<16>(2)?;
        let has_digest = sha1.is_some() || md5.is_some();
        let url = self.urls(project, automatic, has_digest);
        let manual_url = has_digest.then(|| project.manual_page(self.id)).flatten();
        if url.is_empty() && manual_url.is_none() {
            return Err(PackError::CurseForge(format!(
                "{} has no automatic source or verifiable manual download",
                self.file_name
            )));
        }
        Ok(Download {
            path: format!("{}/{}", project.folder()?, self.file_name),
            urls: url,
            size_hint: Some(self.file_length),
            sha1,
            sha512: None,
            md5,
            manual_url,
        })
    }

    fn urls(&self, project: &Project, automatic: bool, has_digest: bool) -> Vec<String> {
        if !self.is_available || (!automatic && project.allow_mod_distribution == Some(false)) {
            return Vec::new();
        }
        let provided = self
            .download_url
            .as_deref()
            .and_then(|url| Url::parse(url.trim()).ok())
            .filter(allows_cdn);
        if let Some(provided) = provided {
            return vec![provided.into()];
        }
        if automatic && has_digest {
            return vec![cdn_url(self.id, &self.file_name)];
        }
        Vec::new()
    }

    fn hash<const BYTES: usize>(&self, algorithm: u32) -> Result<Option<[u8; BYTES]>, PackError> {
        let Some(hash) = self.hashes.iter().find(|hash| hash.algo == algorithm) else {
            return Ok(None);
        };
        let mut bytes = [0; BYTES];
        hex::decode_to_slice(&hash.value, &mut bytes)
            .map_err(|_| PackError::CurseForge(format!("invalid digest for {}", self.file_name)))?;
        Ok(Some(bytes))
    }
}

impl Project {
    fn folder(&self) -> Result<&'static str, PackError> {
        match self.class_id {
            Some(6) => Ok("mods"),
            Some(12) => Ok("resourcepacks"),
            Some(6552) => Ok("shaderpacks"),
            Some(6945) => Ok("datapacks"),
            class => Err(PackError::Unsupported(format!(
                "CurseForge project {} has unsupported content class {class:?}",
                self.id
            ))),
        }
    }

    fn manual_page(&self, file_id: u32) -> Option<String> {
        let mut url = self
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
}
