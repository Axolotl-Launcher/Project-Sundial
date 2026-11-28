//! CurseForge metadata shared by the public catalog and modpack planner.

use serde::Deserialize;

pub const MINECRAFT_GAME_ID: u32 = 432;

/// Resolves pinned manifest entries without giving the importer credentials or HTTP access.
pub trait ModpackCatalog {
    fn get_mods(&self, ids: &[u32]) -> impl Future<Output = Result<Vec<Mod>, CatalogError>>;
    fn get_files(&self, ids: &[u32]) -> impl Future<Output = Result<Vec<File>, CatalogError>>;
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mod {
    pub id: u32,
    pub game_id: u32,
    pub name: String,
    pub slug: String,
    pub summary: String,
    pub class_id: Option<u32>,
    pub allow_mod_distribution: Option<bool>,
    pub links: Links,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    pub website_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    pub id: u32,
    pub mod_id: u32,
    pub game_id: u32,
    pub display_name: String,
    pub file_name: String,
    pub file_length: u64,
    pub download_url: Option<String>,
    pub is_available: bool,
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub hashes: Vec<FileHash>,
}

#[derive(Debug, Deserialize)]
pub struct FileHash {
    pub algo: u32,
    pub value: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, thiserror::Error)]
pub enum CatalogError {
    #[error(
        "CurseForge is disabled because no API key is configured. Set CURSEFORGE_API_KEY before starting the launcher."
    )]
    MissingApiKey,
    #[error("The configured CurseForge API key is invalid.")]
    InvalidApiKey,
    #[error("Invalid CurseForge catalog request.")]
    InvalidRequest,
    #[error("CurseForge denied access (HTTP {status}).")]
    AccessDenied { status: u16 },
    #[error("CurseForge did not find the requested resource.")]
    NotFound,
    #[error("CurseForge rate limit reached; retry in {retry_after_seconds} seconds.")]
    RateLimited { retry_after_seconds: u32 },
    #[error("CurseForge returned HTTP {status}.")]
    Http { status: u16 },
    #[error("The CurseForge request timed out.")]
    Timeout,
    #[error("Could not reach CurseForge.")]
    Transport,
    #[error("CurseForge returned an invalid or oversized response.")]
    InvalidResponse,
}
