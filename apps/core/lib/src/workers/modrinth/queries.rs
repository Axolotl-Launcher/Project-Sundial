use serde::{Deserialize, Serialize};

use super::{
    CatalogError, MAX_QUERY_BYTES, ProjectKey, VersionId, VersionKey, VersionType, filters,
    keys::{MAX_BATCH, valid_key},
};

const MAX_KEY_BYTES: usize = 128;

pub(super) fn validate_query(query: &str, allow_empty: bool) -> Result<(), CatalogError> {
    if query.len() > MAX_QUERY_BYTES
        || (!allow_empty && query.trim().is_empty())
        || query.chars().any(char::is_control)
    {
        return Err(CatalogError::InvalidRequest);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Mod,
    Modpack,
    Shader,
    ResourcePack,
    DataPack,
    Plugin,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionPreferences {
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
}

impl ResolutionPreferences {
    fn validate(&self) -> Result<(), CatalogError> {
        filters::validate_filters(&self.loaders, &self.game_versions)
    }
}

/// Requests a dependency plan; the worker does not install or download its files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentRequest {
    pub project_id: ProjectKey,
    pub version_id: Option<VersionId>,
    pub content_type: ContentType,
    pub selected: ResolutionPreferences,
    pub target: ResolutionPreferences,
    pub existing_project_ids: Vec<ProjectKey>,
    pub update_channel: Option<VersionType>,
}

impl ContentRequest {
    pub(super) fn validate(&self) -> Result<(), CatalogError> {
        self.project_id.validate()?;
        if let Some(version) = &self.version_id {
            version.validate()?;
        }
        if self.existing_project_ids.len() > MAX_BATCH {
            return Err(CatalogError::InvalidRequest);
        }
        self.existing_project_ids
            .iter()
            .try_for_each(ProjectKey::validate)?;
        self.selected.validate()?;
        self.target.validate()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Tags {
    Categories,
    Loaders,
    Licenses,
    DonationPlatforms,
    LinkPlatforms,
    ReportTypes,
    ProjectTypes,
    SideTypes,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ForgeLoader {
    Forge,
    NeoForge,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CountryCode(String);

impl CountryCode {
    pub fn new(value: impl Into<String>) -> Result<Self, CatalogError> {
        let value = Self(value.into().to_ascii_uppercase());
        value.validate()?;
        Ok(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(super) fn validate(&self) -> Result<(), CatalogError> {
        (self.0.len() == 2 && self.0.bytes().all(|byte| byte.is_ascii_uppercase()))
            .then_some(())
            .ok_or(CatalogError::InvalidRequest)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevenueRange {
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub start: Option<time::OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub end: Option<time::OffsetDateTime>,
}

impl RevenueRange {
    pub(super) fn validate(&self) -> Result<(), CatalogError> {
        let now = time::OffsetDateTime::now_utc();
        let start = self.start.unwrap_or(now - time::Duration::days(30));
        let end = self.end.unwrap_or(now);
        if end < start || (end - start).whole_days() > 366 {
            return Err(CatalogError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MavenCoordinate {
    pub project: ProjectKey,
    pub version: VersionKey,
    pub filename: String,
}

impl MavenCoordinate {
    pub(super) fn validate(&self) -> Result<(), CatalogError> {
        self.project.validate()?;
        self.version.validate()?;
        valid_key(&self.filename, b"._+-", MAX_KEY_BYTES)
            .then_some(())
            .ok_or(CatalogError::InvalidRequest)
    }
}
