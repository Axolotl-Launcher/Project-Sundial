use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axlauncher_storage::{DataDir, DataPath};

use crate::modrinth::*;
use crate::{Event, Image, Lease, Service};

type Worker = Arc<Service<Reply>>;
type Slots = HashMap<[PathBuf; 3], Worker>;

static WORKERS: OnceLock<Workers> = OnceLock::new();
static COOLDOWN: tokio::sync::Mutex<Option<Instant>> = tokio::sync::Mutex::const_new(None);

struct Workers {
    image: Image,
    executable: Result<PathBuf, String>,
    slots: Mutex<Option<Slots>>,
}

impl Workers {
    fn new(image: Image, executable: Result<PathBuf, String>) -> Self {
        Self {
            image,
            executable,
            slots: Mutex::new(Some(HashMap::new())),
        }
    }

    fn for_data(&self, data: &DataDir) -> Result<Worker, CatalogError> {
        let mut slots = self.slots.lock().expect("Modrinth workers");
        let slots = slots.as_mut().ok_or(CatalogError::WorkerUnavailable)?;
        slots.retain(|_, worker| Arc::strong_count(worker) > 1 || !worker.is_reaped());
        let scope = [DataPath::Root, DataPath::Cache, DataPath::Profiles]
            .map(|path| data.path(path).to_owned());
        let worker = slots
            .entry(scope)
            .or_insert_with(|| Arc::new(Service::new(self.image, self.executable.clone())));
        Ok(Arc::clone(worker))
    }

    fn take(&self) -> Vec<Worker> {
        self.slots
            .lock()
            .expect("Modrinth workers")
            .take()
            .unwrap_or_default()
            .into_values()
            .collect()
    }
}

pub(crate) fn configure(image: Image, executable: Result<PathBuf, String>) {
    let _ = WORKERS.set(Workers::new(image, executable));
}

pub(crate) fn take_workers() -> Vec<Worker> {
    WORKERS.get().map_or_else(Vec::new, Workers::take)
}

/// Catalog access bound to a launcher data snapshot. Poll requests off the UI thread.
#[derive(Clone)]
pub struct DesktopModrinth {
    data: DataDir,
    worker: Worker,
}

impl DesktopModrinth {
    /// Resolves the scoped worker slot without starting a process.
    pub fn new(data: DataDir) -> Result<Self, CatalogError> {
        let workers = WORKERS.get().ok_or(CatalogError::WorkerUnavailable)?;
        let worker = workers.for_data(&data)?;
        Ok(Self { data, worker })
    }

    /// Keeps a started worker alive for a group of requests in this scope.
    pub fn hold(&self) -> ModrinthLease<'_> {
        ModrinthLease {
            _lease: self.worker.hold(),
        }
    }

    pub async fn search(&self, search: Search) -> Result<SearchResult, CatalogError> {
        match self.execute(Operation::Search(search)).await? {
            Payload::Search(page) => Ok(SearchResult::Offset(page)),
            Payload::ModernSearch(page) => Ok(SearchResult::Pages(page)),
            _ => Err(CatalogError::WorkerProtocol),
        }
    }

    async fn execute(&self, operation: Operation) -> Result<Payload, CatalogError> {
        operation.validate()?;
        execute_with_cooldown(&COOLDOWN, async {
            let mut request = self
                .worker
                .request(&self.data, Command::V1(operation))
                .map_err(|_| CatalogError::WorkerUnavailable)?;
            receive(async || request.next().await).await
        })
        .await
    }
}

macro_rules! public_lookups {
    ($($method:ident($($arg:ident: $input:ty),*) -> $output:ty = $operation:expr => $payload:ident;)*) => {
        impl DesktopModrinth {
            $(pub async fn $method(&self, $($arg: $input),*) -> Result<$output, CatalogError> {
                match self.execute($operation).await? {
                    Payload::$payload(result) => Ok(result),
                    _ => Err(CatalogError::WorkerProtocol),
                }
            })*
        }
    };
}

type ByHash<T> = std::collections::BTreeMap<String, T>;

public_lookups! {
    get_project(key: ProjectKey) -> Project = Operation::Project(key) => Project;
    get_projects(keys: Batch<ProjectKey>) -> Vec<Project> = Operation::Projects(keys) => Projects;
    random_projects(count: u8) -> Vec<Project> = Operation::RandomProjects(count) => Projects;
    check_project(key: ProjectKey) -> ProjectCheck = Operation::CheckProject(key) => ProjectCheck;
    project_dependencies(key: ProjectKey) -> Dependencies = Operation::Dependencies(key) => Dependencies;
    list_versions(query: ProjectVersions) -> Vec<Version> = Operation::Versions(query) => Versions;
    get_version(id: VersionId) -> Version = Operation::Version(id) => Version;
    get_project_version(key: ProjectKey, version: VersionKey) -> Version = Operation::ProjectVersion(key, version) => Version;
    get_versions(ids: Batch<VersionId>) -> Vec<Version> = Operation::BatchVersions(ids) => Versions;
    find_versions(query: VersionSearch) -> Vec<Version> = Operation::FindVersions(query) => Versions;
    version_by_hash(hash: HashLookup) -> Version = Operation::FileVersion(hash) => Version;
    file_location(hash: HashLookup) -> Redirect = Operation::FileDownload(hash) => Download;
    latest_version_by_hash(hash: HashLookup, filters: UpdateFilters) -> Version = Operation::FileUpdate(hash, filters) => Version;
    projects_by_hash(hashes: HashBatch) -> ByHash<Project> = Operation::ProjectsByHash(hashes) => ProjectsByHash;
    versions_by_hash(hashes: HashBatch) -> ByHash<Version> = Operation::VersionsByHash(hashes) => VersionsByHash;
    latest_versions_by_hash(query: BulkUpdate) -> ByHash<Version> = Operation::UpdatesByHash(query) => VersionsByHash;
    all_matching_updates(query: BulkUpdate) -> ByHash<Vec<Version>> = Operation::AllUpdatesByHash(query) => UpdatesByHash;
    individual_updates(query: IndividualUpdates) -> ByHash<Version> = Operation::IndividualUpdates(query) => VersionsByHash;
    get_user(key: UserKey) -> User = Operation::User(key) => User;
    get_users(keys: Batch<UserKey>) -> Vec<User> = Operation::Users(keys) => Users;
    search_users(query: String) -> Vec<UserHit> = Operation::SearchUsers(query) => SearchUsers;
    user_projects(key: UserKey) -> Vec<Project> = Operation::UserProjects(key) => Projects;
    user_all_projects(key: UserKey) -> AllProjects = Operation::UserAllProjects(key) => AllProjects;
    project_members(key: ProjectKey) -> Vec<Member> = Operation::ProjectMembers(key) => Members;
    team_members(id: TeamId) -> Vec<Member> = Operation::TeamMembers(id) => Members;
    teams(ids: Batch<TeamId>) -> Vec<Vec<Member>> = Operation::Teams(ids) => Teams;
    get_organization(key: ProjectKey) -> Organization = Operation::Organization(key) => Organization;
    get_organizations(keys: Batch<ProjectKey>) -> Vec<Organization> = Operation::Organizations(keys) => Organizations;
    project_organization(key: ProjectKey) -> Organization = Operation::ProjectOrganization(key) => Organization;
    organization_projects(key: ProjectKey) -> Vec<ModernProject> = Operation::OrganizationProjects(key) => OrganizationProjects;
    organization_members(key: ProjectKey) -> Vec<Member> = Operation::OrganizationMembers(key) => Members;
    user_organizations(key: UserKey) -> Vec<Organization> = Operation::UserOrganizations(key) => Organizations;
    get_collection(id: CollectionId) -> Collection = Operation::Collection(id) => Collection;
    get_collections(ids: Batch<CollectionId>) -> Vec<Collection> = Operation::Collections(ids) => Collections;
    user_collections(key: UserKey) -> Vec<Collection> = Operation::UserCollections(key) => Collections;
    resolve_content(query: ContentRequest) -> ContentPlan = Operation::ResolveContent(query) => Content;
    project_disclosures(key: ProjectKey) -> Disclosures = Operation::Disclosures(key) => Disclosures;
    categories() -> Vec<Category> = Operation::Tags(Tags::Categories) => Categories;
    loaders() -> Vec<Loader> = Operation::Tags(Tags::Loaders) => Loaders;
    game_versions(filter: GameVersionFilter) -> Vec<GameVersion> = Operation::GameVersions(filter) => GameVersions;
    licenses() -> Vec<License> = Operation::Tags(Tags::Licenses) => Licenses;
    license_text(id: LicenseId) -> LicenseText = Operation::License(id) => LicenseText;
    donation_platforms() -> Vec<DonationPlatform> = Operation::Tags(Tags::DonationPlatforms) => DonationPlatforms;
    link_platforms() -> Vec<LinkPlatform> = Operation::Tags(Tags::LinkPlatforms) => LinkPlatforms;
    report_types() -> Vec<String> = Operation::Tags(Tags::ReportTypes) => TagNames;
    project_types() -> Vec<String> = Operation::Tags(Tags::ProjectTypes) => TagNames;
    side_types() -> Vec<String> = Operation::Tags(Tags::SideTypes) => TagNames;
    loader_values(query: LoaderFieldQuery) -> Vec<LoaderValue> = Operation::LoaderField(query) => LoaderValues;
    forge_updates(key: ProjectKey, loader: ForgeLoader) -> ForgeUpdates = Operation::ForgeUpdates(key, loader) => ForgeUpdates;
    statistics() -> Statistics = Operation::Statistics => Statistics;
    analytics_events() -> Vec<AnalyticsEvent> = Operation::AnalyticsEvents => AnalyticsEvents;
    payment_methods(country: Option<CountryCode>) -> Vec<PaymentMethod> = Operation::PaymentMethods(country) => PaymentMethods;
    platform_revenue(range: RevenueRange) -> PlatformRevenue = Operation::PlatformRevenue(range) => PlatformRevenue;
    maven_metadata(key: ProjectKey) -> MavenResult<MavenMetadata> = Operation::MavenMetadata(key) => MavenMetadata;
    maven_artifact(coordinate: MavenCoordinate) -> MavenResult<MavenPom> = Operation::MavenArtifact(coordinate) => MavenArtifact;
    maven_head(coordinate: MavenCoordinate) -> MavenResult<()> = Operation::MavenHead(coordinate) => MavenHead;
    maven_hash(coordinate: MavenCoordinate, algorithm: HashAlgorithm) -> MavenResult<String> = Operation::MavenHash(coordinate, algorithm) => MavenHash;
}

/// An optional process lease for a group of Modrinth requests.
pub struct ModrinthLease<'a> {
    _lease: Lease<'a, Reply>,
}

async fn execute_with_cooldown(
    cooldown: &tokio::sync::Mutex<Option<Instant>>,
    request: impl Future<Output = Result<Reply, CatalogError>>,
) -> Result<Payload, CatalogError> {
    let deadline = *cooldown.lock().await;
    check_cooldown(deadline, Instant::now())?;
    let (delay, result) = request.await?.into_parts();
    if let Some(seconds) = delay {
        let retry_at = Instant::now() + Duration::from_secs(seconds.into());
        let mut deadline = cooldown.lock().await;
        *deadline = (*deadline).max(Some(retry_at));
    }
    result
}

fn check_cooldown(cooldown: Option<Instant>, now: Instant) -> Result<(), CatalogError> {
    if let Some(until) = cooldown.filter(|until| *until > now) {
        let remaining = until - now;
        let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
        return Err(CatalogError::RateLimited {
            retry_after_seconds: u32::try_from(seconds).unwrap_or(u32::MAX),
        });
    }
    Ok(())
}

async fn receive(
    mut next: impl AsyncFnMut() -> Option<Event<Reply>>,
) -> Result<Reply, CatalogError> {
    let reply = match next().await {
        Some(Event::Reply(reply)) => reply,
        Some(Event::Disconnected) | None => return Err(CatalogError::WorkerUnavailable),
        _ => return Err(CatalogError::WorkerProtocol),
    };
    match next().await {
        Some(Event::Finished(Ok(()))) => Ok(reply),
        Some(Event::Disconnected) | None => Err(CatalogError::WorkerUnavailable),
        _ => Err(CatalogError::WorkerProtocol),
    }
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/src/workers/modrinth_client.rs"]
mod tests;
