//! Typed requests and results for the embedded Modrinth public catalog.

use serde::{Deserialize, Serialize};

#[path = "modrinth_client.rs"]
mod client;
#[path = "modrinth/query_filters.rs"]
mod filters;
#[path = "modrinth/query_hashes.rs"]
mod hashes;
#[path = "modrinth/keys.rs"]
mod keys;
#[path = "modrinth/models.rs"]
mod models;
#[path = "modrinth/queries.rs"]
mod queries;
pub use client::{DesktopModrinth, ModrinthLease};
pub(crate) use client::{configure, take_workers};
pub use filters::*;
pub use hashes::*;
pub use keys::*;
pub use models::*;
pub use queries::*;

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
    Projects(Batch<ProjectKey>),
    RandomProjects(u8),
    CheckProject(ProjectKey),
    Dependencies(ProjectKey),
    Versions(ProjectVersions),
    Version(VersionId),
    ProjectVersion(ProjectKey, VersionKey),
    BatchVersions(Batch<VersionId>),
    FindVersions(VersionSearch),
    FileVersion(HashLookup),
    FileDownload(HashLookup),
    FileUpdate(HashLookup, UpdateFilters),
    ProjectsByHash(HashBatch),
    VersionsByHash(HashBatch),
    UpdatesByHash(BulkUpdate),
    AllUpdatesByHash(BulkUpdate),
    IndividualUpdates(IndividualUpdates),
    User(UserKey),
    Users(Batch<UserKey>),
    SearchUsers(String),
    UserProjects(UserKey),
    UserAllProjects(UserKey),
    ProjectMembers(ProjectKey),
    TeamMembers(TeamId),
    Teams(Batch<TeamId>),
    Organization(ProjectKey),
    Organizations(Batch<ProjectKey>),
    ProjectOrganization(ProjectKey),
    OrganizationProjects(ProjectKey),
    OrganizationMembers(ProjectKey),
    UserOrganizations(UserKey),
    Collection(CollectionId),
    Collections(Batch<CollectionId>),
    UserCollections(UserKey),
    ResolveContent(ContentRequest),
    Disclosures(ProjectKey),
    Tags(Tags),
    License(LicenseId),
    GameVersions(GameVersionFilter),
    LoaderField(LoaderFieldQuery),
    ForgeUpdates(ProjectKey, ForgeLoader),
    Statistics,
    AnalyticsEvents,
    PaymentMethods(Option<CountryCode>),
    PlatformRevenue(RevenueRange),
    MavenMetadata(ProjectKey),
    MavenArtifact(MavenCoordinate),
    MavenHead(MavenCoordinate),
    MavenHash(MavenCoordinate, HashAlgorithm),
}

impl Operation {
    pub fn validate(&self) -> Result<(), CatalogError> {
        match self {
            Self::Search(search) => search.validate(),
            Self::Project(key)
            | Self::CheckProject(key)
            | Self::Dependencies(key)
            | Self::ProjectMembers(key)
            | Self::Organization(key)
            | Self::ProjectOrganization(key)
            | Self::OrganizationProjects(key)
            | Self::OrganizationMembers(key)
            | Self::Disclosures(key)
            | Self::ForgeUpdates(key, _)
            | Self::MavenMetadata(key) => key.validate(),
            Self::Projects(keys) | Self::Organizations(keys) => keys.validate(ProjectKey::validate),
            Self::RandomProjects(count) if *count <= MAX_SEARCH_LIMIT => Ok(()),
            Self::RandomProjects(_) => Err(CatalogError::InvalidRequest),
            Self::Versions(query) => query.validate(),
            Self::Version(id) => id.validate(),
            Self::ProjectVersion(project, version) => {
                project.validate().and_then(|()| version.validate())
            }
            Self::BatchVersions(ids) => ids.validate(VersionId::validate),
            Self::FindVersions(query) => query.validate(),
            Self::FileVersion(hash) | Self::FileDownload(hash) => hash.validate(),
            Self::FileUpdate(hash, filters) => hash.validate().and_then(|()| filters.validate()),
            Self::ProjectsByHash(hashes) | Self::VersionsByHash(hashes) => hashes.validate(),
            Self::UpdatesByHash(query) | Self::AllUpdatesByHash(query) => query.validate(),
            Self::IndividualUpdates(query) => query.validate(),
            Self::User(key)
            | Self::UserProjects(key)
            | Self::UserAllProjects(key)
            | Self::UserOrganizations(key)
            | Self::UserCollections(key) => key.validate(),
            Self::Users(keys) => keys.validate(UserKey::validate),
            Self::SearchUsers(query) => validate_query(query, false),
            Self::TeamMembers(id) => id.validate(),
            Self::Teams(ids) => ids.validate(TeamId::validate),
            Self::Collection(id) => id.validate(),
            Self::Collections(ids) => ids.validate(CollectionId::validate),
            Self::ResolveContent(query) => query.validate(),
            Self::License(id) => id.validate(),
            Self::PaymentMethods(Some(country)) => country.validate(),
            Self::PlatformRevenue(range) => range.validate(),
            Self::MavenArtifact(coordinate)
            | Self::MavenHead(coordinate)
            | Self::MavenHash(coordinate, _) => coordinate.validate(),
            Self::Statistics
            | Self::Tags(_)
            | Self::AnalyticsEvents
            | Self::PaymentMethods(None)
            | Self::GameVersions(_)
            | Self::LoaderField(_) => Ok(()),
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
    #[serde(default)]
    pub sort: SearchSort,
    #[serde(default)]
    pub facets: Vec<Vec<Facet>>,
    #[serde(default)]
    pub method: SearchMethod,
}

impl Search {
    /// An empty query browses the catalog; the provider caps each page at 100 hits.
    pub fn new(query: impl Into<String>, offset: u32, limit: u8) -> Result<Self, CatalogError> {
        let search = Self {
            query: query.into(),
            offset,
            limit,
            sort: SearchSort::Relevance,
            facets: Vec::new(),
            method: SearchMethod::Get,
        };
        search.validate()?;
        Ok(search)
    }

    pub fn parameters(&self) -> (&str, u32, u8) {
        (&self.query, self.offset, self.limit)
    }

    fn validate(&self) -> Result<(), CatalogError> {
        validate_query(&self.query, true)?;
        if self.limit > MAX_SEARCH_LIMIT
            || matches!(self.method, SearchMethod::Post) && self.limit == 0
            || self.facets.len() > 10
        {
            return Err(CatalogError::InvalidRequest);
        }
        if self
            .facets
            .iter()
            .any(|group| group.is_empty() || group.len() > 8)
        {
            return Err(CatalogError::InvalidRequest);
        }
        self.facets.iter().flatten().try_for_each(Facet::validate)
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
    pub team: Option<String>,
    pub organization: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
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
    pub changelog: Option<String>,
    pub featured: Option<bool>,
    pub downloads: Option<u64>,
    #[serde(default)]
    pub dependencies: Vec<VersionDependency>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
    ModernSearch(ModernSearchPage),
    Project(Project),
    Projects(Vec<Project>),
    ProjectCheck(ProjectCheck),
    Dependencies(Dependencies),
    Version(Version),
    Versions(Vec<Version>),
    ProjectsByHash(std::collections::BTreeMap<String, Project>),
    VersionsByHash(std::collections::BTreeMap<String, Version>),
    UpdatesByHash(std::collections::BTreeMap<String, Vec<Version>>),
    User(User),
    Users(Vec<User>),
    SearchUsers(Vec<UserHit>),
    AllProjects(AllProjects),
    Members(Vec<Member>),
    Teams(Vec<Vec<Member>>),
    Organization(Organization),
    Organizations(Vec<Organization>),
    OrganizationProjects(Vec<ModernProject>),
    Collection(Collection),
    Collections(Vec<Collection>),
    Content(ContentPlan),
    Disclosures(Disclosures),
    Categories(Vec<Category>),
    Loaders(Vec<Loader>),
    GameVersions(Vec<GameVersion>),
    Licenses(Vec<License>),
    LicenseText(LicenseText),
    DonationPlatforms(Vec<DonationPlatform>),
    LinkPlatforms(Vec<LinkPlatform>),
    TagNames(Vec<String>),
    LoaderValues(Vec<LoaderValue>),
    ForgeUpdates(ForgeUpdates),
    Statistics(Statistics),
    AnalyticsEvents(Vec<AnalyticsEvent>),
    PaymentMethods(Vec<PaymentMethod>),
    PlatformRevenue(PlatformRevenue),
    Download(Redirect),
    MavenMetadata(MavenResult<MavenMetadata>),
    MavenArtifact(MavenResult<MavenPom>),
    MavenHead(MavenResult<()>),
    MavenHash(MavenResult<String>),
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
