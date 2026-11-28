//! Minecraft's CurseForge catalog via a fixed set of authenticated v1 routes.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use axlauncher_network_spi::auth::{ApiKey, AuthNetwork, Body, Request, TransportError};
use axlauncher_spi::curseforge::{CatalogError, File, MINECRAFT_GAME_ID, Mod, ModpackCatalog};
use serde::{Deserialize, de::DeserializeOwned};
use url::Url;

mod query;
pub use query::{Category, Page, PageRequest, Search};

const API_ROOT: &str = "https://api.curseforge.com/v1";
const USER_AGENT: &str = concat!(
    "Axolotl-Launcher/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/Axolotl-Launcher/Project-Sundial)"
);
const FILE_BATCH_LIMIT: usize = 25;
const MOD_BATCH_LIMIT: usize = 10;
const DEFAULT_RETRY_SECONDS: u32 = 60;
const MAX_RETRY_SECONDS: u32 = 3600;

/// Shares the request policy used by both browsing and archive installation.
pub struct Catalog<N> {
    network: N,
    key: ApiKey,
    cooldown: Mutex<Option<Instant>>,
}

impl<N: AuthNetwork> Catalog<N> {
    pub fn new(network: N, key: ApiKey) -> Self {
        Self {
            network,
            key,
            cooldown: Mutex::new(None),
        }
    }

    pub async fn search(&self, search: Search) -> Result<Page<Mod>, CatalogError> {
        let page = search.page();
        let mut url = endpoint("/mods/search");
        search.append(&mut url, MINECRAFT_GAME_ID);
        self.send::<Page<_>>(url, None).await?.validate(page)
    }

    pub async fn get_mod(&self, mod_id: u32) -> Result<Mod, CatalogError> {
        self.get(&mod_path(mod_id)?).await
    }

    pub async fn list_files(
        &self,
        mod_id: u32,
        page: PageRequest,
    ) -> Result<Page<File>, CatalogError> {
        let mut url = endpoint(&format!("{}/files", mod_path(mod_id)?));
        page.append(&mut url);
        self.send::<Page<_>>(url, None).await?.validate(page)
    }

    pub async fn get_file(&self, mod_id: u32, file_id: u32) -> Result<File, CatalogError> {
        self.get(&file_path(mod_id, file_id)?).await
    }

    pub async fn download_url(
        &self,
        mod_id: u32,
        file_id: u32,
    ) -> Result<Option<Url>, CatalogError> {
        let url: Option<String> = self
            .get(&format!("{}/download-url", file_path(mod_id, file_id)?))
            .await?;
        url.map(|value| {
            Url::parse(&value)
                .ok()
                .filter(|url| {
                    url.scheme() == "https"
                        && url.host_str().is_some()
                        && url.username().is_empty()
                        && url.password().is_none()
                })
                .ok_or(CatalogError::InvalidResponse)
        })
        .transpose()
    }

    /// Project descriptions are untrusted provider HTML.
    pub async fn description(&self, mod_id: u32) -> Result<String, CatalogError> {
        self.get(&format!("{}/description", mod_path(mod_id)?))
            .await
    }

    /// File changelogs are untrusted provider HTML.
    pub async fn changelog(&self, mod_id: u32, file_id: u32) -> Result<String, CatalogError> {
        self.get(&format!("{}/changelog", file_path(mod_id, file_id)?))
            .await
    }

    pub async fn categories(&self, class_id: Option<u32>) -> Result<Vec<Category>, CatalogError> {
        let mut url = endpoint("/categories");
        url.query_pairs_mut()
            .append_pair("gameId", &MINECRAFT_GAME_ID.to_string());
        if let Some(class_id) = class_id {
            validate_id(class_id)?;
            url.query_pairs_mut()
                .append_pair("classId", &class_id.to_string());
        }
        self.data(url, None).await
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, CatalogError> {
        self.data(endpoint(path), None).await
    }

    async fn data<T: DeserializeOwned>(
        &self,
        url: Url,
        body: Option<Body>,
    ) -> Result<T, CatalogError> {
        let response: Envelope<T> = self.send(url, body).await?;
        Ok(response.data)
    }

    async fn send<T: DeserializeOwned>(
        &self,
        url: Url,
        body: Option<Body>,
    ) -> Result<T, CatalogError> {
        if let Some(retry_after_seconds) = self.cooldown_seconds() {
            return Err(CatalogError::RateLimited {
                retry_after_seconds,
            });
        }
        let mut request = Request::new(url.as_str(), USER_AGENT).with_api_key(&self.key);
        request.body = body;
        let response = self.network.send(request).await.map_err(transport_error)?;
        match response.status {
            200 => {
                serde_json::from_slice(&response.body).map_err(|_| CatalogError::InvalidResponse)
            }
            status @ (401 | 403) => Err(CatalogError::AccessDenied { status }),
            404 => Err(CatalogError::NotFound),
            429 => Err(self.rate_limited(response.retry_after_seconds)),
            status => Err(CatalogError::Http { status }),
        }
    }

    fn cooldown_seconds(&self) -> Option<u32> {
        let until = (*self.cooldown.lock().expect("CurseForge cooldown"))?;
        let remaining = until.checked_duration_since(Instant::now())?;
        Some(remaining.as_secs() as u32 + u32::from(remaining.subsec_nanos() != 0))
    }

    fn rate_limited(&self, seconds: Option<u32>) -> CatalogError {
        let retry_after_seconds = seconds
            .unwrap_or(DEFAULT_RETRY_SECONDS)
            .clamp(1, MAX_RETRY_SECONDS);
        let until = Instant::now() + Duration::from_secs(u64::from(retry_after_seconds));
        let mut cooldown = self.cooldown.lock().expect("CurseForge cooldown");
        *cooldown = (*cooldown).max(Some(until));
        CatalogError::RateLimited {
            retry_after_seconds,
        }
    }

    async fn batch<T: DeserializeOwned>(
        &self,
        path: &str,
        field: &str,
        ids: &[u32],
        limit: usize,
    ) -> Result<Vec<T>, CatalogError> {
        if ids.contains(&0) {
            return Err(CatalogError::InvalidRequest);
        }
        let url = endpoint(path);
        let mut results = Vec::new();
        for chunk in ids.chunks(limit) {
            let body = Body::Json(serde_json::json!({ field: chunk }).to_string());
            let files: Vec<T> = self.data(url.clone(), Some(body)).await?;
            if files.len() > chunk.len() {
                return Err(CatalogError::InvalidResponse);
            }
            results.extend(files);
        }
        Ok(results)
    }
}

impl<N: AuthNetwork> ModpackCatalog for Catalog<N> {
    async fn get_mods(&self, ids: &[u32]) -> Result<Vec<Mod>, CatalogError> {
        self.batch("/mods", "modIds", ids, MOD_BATCH_LIMIT).await
    }

    async fn get_files(&self, ids: &[u32]) -> Result<Vec<File>, CatalogError> {
        self.batch("/mods/files", "fileIds", ids, FILE_BATCH_LIMIT)
            .await
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

fn endpoint(path: &str) -> Url {
    Url::parse(&format!("{API_ROOT}{path}")).expect("fixed CurseForge endpoint")
}

fn validate_id(id: u32) -> Result<(), CatalogError> {
    if id == 0 {
        return Err(CatalogError::InvalidRequest);
    }
    Ok(())
}

fn mod_path(mod_id: u32) -> Result<String, CatalogError> {
    validate_id(mod_id)?;
    Ok(format!("/mods/{mod_id}"))
}

fn file_path(mod_id: u32, file_id: u32) -> Result<String, CatalogError> {
    validate_id(file_id)?;
    Ok(format!("{}/files/{file_id}", mod_path(mod_id)?))
}

fn transport_error(error: TransportError) -> CatalogError {
    match error {
        TransportError::Unavailable => CatalogError::Transport,
        TransportError::Timeout => CatalogError::Timeout,
        TransportError::InvalidResponse => CatalogError::InvalidResponse,
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/content/src/curseforge.rs"]
mod tests;
