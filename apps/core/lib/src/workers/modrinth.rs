//! Typed requests and results for the embedded Modrinth public catalog.

use serde::{Deserialize, Serialize};

#[path = "modrinth_client.rs"]
mod client;
pub use client::{DesktopModrinth, ModrinthLease};
pub(crate) use client::{configure, take_workers};

const MAX_QUERY_BYTES: usize = 512;
const MAX_PROJECT_KEY_BYTES: usize = 128;
const MAX_SEARCH_LIMIT: u8 = 100;

/// The catalog wire format evolves explicitly with the embedded image.
#[derive(Debug, Serialize, Deserialize)]
pub enum Command {
    V1(Operation),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Operation {
    Search(Search),
    Project(ProjectKey),
    Versions(ProjectKey),
}

impl Operation {
    pub fn validate(&self) -> Result<(), CatalogError> {
        match self {
            Self::Search(search) => search.validate(),
            Self::Project(key) | Self::Versions(key) => key.validate(),
        }
    }
}

/// A search query and its zero-based result offset.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Search {
    query: String,
    offset: u32,
    limit: u8,
}

impl Search {
    /// An empty query browses the catalog; a page contains 1 to 100 hits.
    pub fn new(query: impl Into<String>, offset: u32, limit: u8) -> Result<Self, CatalogError> {
        let search = Self {
            query: query.into(),
            offset,
            limit,
        };
        search.validate()?;
        Ok(search)
    }

    pub fn parameters(&self) -> (&str, u32, u8) {
        (&self.query, self.offset, self.limit)
    }

    fn validate(&self) -> Result<(), CatalogError> {
        if self.query.len() > MAX_QUERY_BYTES
            || self.query.chars().any(char::is_control)
            || !(1..=MAX_SEARCH_LIMIT).contains(&self.limit)
        {
            return Err(CatalogError::InvalidRequest);
        }
        Ok(())
    }
}

/// A Modrinth project ID or slug, never a path or URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectKey(String);

impl ProjectKey {
    pub fn new(value: impl Into<String>) -> Result<Self, CatalogError> {
        let key = Self(value.into());
        key.validate()?;
        Ok(key)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(&self) -> Result<(), CatalogError> {
        if self.0.is_empty()
            || self.0.len() > MAX_PROJECT_KEY_BYTES
            || matches!(self.0.as_str(), "." | "..")
            || !self
                .0
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(CatalogError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchPage {
    pub hits: Vec<ProjectHit>,
    pub offset: u32,
    pub limit: u8,
    pub total_hits: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub project_type: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub body: String,
    pub project_type: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    pub version_type: VersionType,
    pub date_published: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub files: Vec<VersionFile>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionType {
    Release,
    Beta,
    Alpha,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VersionFile {
    pub filename: String,
    pub url: String,
    pub size: u64,
    pub primary: bool,
    pub hashes: VersionHashes,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VersionHashes {
    pub sha1: Option<String>,
    pub sha512: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum CatalogError {
    #[error("Invalid Modrinth catalog request.")]
    InvalidRequest,
    #[error("The Modrinth project was not found.")]
    NotFound,
    #[error("Modrinth denied access to the requested resource (HTTP {status}).")]
    AccessDenied { status: u16 },
    #[error("Modrinth rate limit reached; retry in {retry_after_seconds} seconds.")]
    RateLimited { retry_after_seconds: u32 },
    #[error("Modrinth returned HTTP {status}.")]
    Http { status: u16 },
    #[error("The Modrinth request timed out.")]
    Timeout,
    #[error("Could not reach Modrinth.")]
    Transport,
    #[error("Modrinth returned an invalid or oversized response.")]
    InvalidResponse,
    #[error("The embedded Modrinth worker is unavailable or disconnected.")]
    WorkerUnavailable,
    #[error("The embedded Modrinth worker sent an invalid reply.")]
    WorkerProtocol,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Payload {
    Search(SearchPage),
    Project(Project),
    Versions(Vec<Version>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    cooldown_seconds: Option<u32>,
    result: Result<Payload, CatalogError>,
}

impl Reply {
    pub fn new(cooldown_seconds: Option<u32>, result: Result<Payload, CatalogError>) -> Self {
        Self {
            cooldown_seconds,
            result,
        }
    }

    pub fn into_parts(self) -> (Option<u32>, Result<Payload, CatalogError>) {
        (self.cooldown_seconds, self.result)
    }
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/src/workers/modrinth.rs"]
mod tests;
