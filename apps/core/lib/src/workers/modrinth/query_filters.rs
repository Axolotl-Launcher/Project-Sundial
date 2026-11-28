use serde::{Deserialize, Serialize};

use super::{Batch, CatalogError, ProjectKey, VersionType, keys::valid_key};

const MAX_FILTER_VALUES: usize = 20;
const MAX_VERSION_VALUE: usize = 64;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    #[default]
    Relevance,
    Downloads,
    Follows,
    Newest,
    Updated,
}

impl SearchSort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::Downloads => "downloads",
            Self::Follows => "follows",
            Self::Newest => "newest",
            Self::Updated => "updated",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum SearchMethod {
    #[default]
    Get,
    Post,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FacetField {
    ProjectType,
    AllProjectTypes,
    Categories,
    Versions,
    OpenSource,
    Environment,
    DisclosureTypes,
    ClientSide,
    ServerSide,
    Title,
    Author,
    Follows,
    ProjectId,
    License,
    Downloads,
    CreatedTimestamp,
    ModifiedTimestamp,
}

impl FacetField {
    fn as_str(self) -> &'static str {
        match self {
            Self::ProjectType => "project_type",
            Self::AllProjectTypes => "all_project_types",
            Self::Categories => "categories",
            Self::Versions => "versions",
            Self::OpenSource => "open_source",
            Self::Environment => "environment",
            Self::DisclosureTypes => "disclosure_types",
            Self::ClientSide => "client_side",
            Self::ServerSide => "server_side",
            Self::Title => "title",
            Self::Author => "author",
            Self::Follows => "follows",
            Self::ProjectId => "project_id",
            Self::License => "license",
            Self::Downloads => "downloads",
            Self::CreatedTimestamp => "created_timestamp",
            Self::ModifiedTimestamp => "modified_timestamp",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Comparison {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

impl Comparison {
    fn as_str(self) -> &'static str {
        match self {
            Self::Equal => ":",
            Self::NotEqual => "!=",
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
            Self::Less => "<",
            Self::LessOrEqual => "<=",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Facet {
    pub field: FacetField,
    pub comparison: Comparison,
    pub value: String,
}

impl Facet {
    pub(crate) fn validate(&self) -> Result<(), CatalogError> {
        let valid = match self.field {
            FacetField::Downloads
            | FacetField::Follows
            | FacetField::CreatedTimestamp
            | FacetField::ModifiedTimestamp => self.value.parse::<u64>().is_ok(),
            FacetField::OpenSource => matches!(self.value.as_str(), "true" | "false"),
            FacetField::Title | FacetField::Author => {
                !self.value.trim().is_empty()
                    && self.value.len() <= MAX_VERSION_VALUE
                    && !self
                        .value
                        .chars()
                        .any(|value| value.is_control() || ":=!<>".contains(value))
            }
            _ => valid_key(&self.value, b"._-+", MAX_VERSION_VALUE),
        };
        if valid {
            Ok(())
        } else {
            Err(CatalogError::InvalidRequest)
        }
    }

    pub fn expression(&self, method: SearchMethod) -> String {
        let field = match (method, self.field) {
            (SearchMethod::Post, FacetField::Versions) => "game_versions",
            (SearchMethod::Post, FacetField::ProjectType) => "project_types",
            (SearchMethod::Post, FacetField::Title) => "name",
            (_, field) => field.as_str(),
        };
        format!("{}{}{}", field, self.comparison.as_str(), self.value)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionFilters {
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
    pub featured: Option<bool>,
    pub version_type: Option<VersionType>,
    pub limit: Option<u8>,
    pub offset: Option<u32>,
    pub include_changelog: bool,
}

impl VersionFilters {
    fn validate(&self) -> Result<(), CatalogError> {
        if self.limit.is_some_and(|limit| limit > 100) {
            return Err(CatalogError::InvalidRequest);
        }
        validate_filters(&self.loaders, &self.game_versions)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectVersions {
    pub project: ProjectKey,
    pub filters: VersionFilters,
}

impl ProjectVersions {
    pub(crate) fn validate(&self) -> Result<(), CatalogError> {
        self.project.validate()?;
        self.filters.validate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionSearch {
    pub projects: Batch<ProjectKey>,
    pub filters: VersionFilters,
}

impl VersionSearch {
    pub(crate) fn validate(&self) -> Result<(), CatalogError> {
        self.projects.validate(ProjectKey::validate)?;
        self.filters.validate()
    }
}

pub(super) fn validate_filters(
    loaders: &[String],
    game_versions: &[String],
) -> Result<(), CatalogError> {
    if loaders.len() > MAX_FILTER_VALUES
        || game_versions.len() > MAX_FILTER_VALUES
        || !loaders
            .iter()
            .all(|value| valid_key(value, b"_-", MAX_VERSION_VALUE))
        || !game_versions
            .iter()
            .all(|value| valid_key(value, b"._-+", MAX_VERSION_VALUE))
    {
        return Err(CatalogError::InvalidRequest);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GameVersionType {
    Release,
    Snapshot,
    Alpha,
    Beta,
}

impl GameVersionType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::Snapshot => "snapshot",
            Self::Alpha => "alpha",
            Self::Beta => "beta",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameVersionFilter {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub version_type: Option<GameVersionType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub major: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoaderFieldQuery {
    GameVersions,
    Environment,
    PackLoaders,
}

impl LoaderFieldQuery {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GameVersions => "game_versions",
            Self::Environment => "environment",
            Self::PackLoaders => "mrpack_loaders",
        }
    }
}
