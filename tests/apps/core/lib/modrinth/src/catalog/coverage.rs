use std::time::Duration;

use axlauncher_worker::modrinth::*;

use super::{TIMEOUT, mock, response};

const PROJECT: &str = r#"{"id":"AABBCCDD","slug":"demo","title":"Demo","description":"summary","body":"details","project_type":"mod","downloads":4,"team":"QQRRSSTT"}"#;
const VERSION: &str = r#"{"id":"IIJJKKLL","project_id":"AABBCCDD","name":"Release","version_number":"1.0+client","version_type":"release","date_published":"2026-11-28T00:00:00Z","game_versions":["1.21.1"],"loaders":["fabric"],"files":[]}"#;
const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
const CDN: &str = "https://cdn.modrinth.com/data/AABBCCDD/versions/IIJJKKLL/demo.jar";

macro_rules! covered {
    ($client:ident, $route:expr, $operation:expr, $payload:ident, redirect) => {{
        let _ = DesktopModrinth::$client;
        verify($route, $operation, stringify!($payload), moved()).await
    }};
    ($client:ident, $route:expr, $operation:expr, $payload:ident, $body:expr) => {{
        let _ = DesktopModrinth::$client;
        verify($route, $operation, stringify!($payload), ok($body)).await
    }};
}

#[path = "coverage_people.rs"]
mod people;
#[path = "coverage_public.rs"]
mod public;

#[tokio::test]
async fn catalog_routes() {
    let project = ProjectKey::new("demo").unwrap();
    let projects = Batch::new(vec![project.clone()]).unwrap();
    let version = VersionId::new("IIJJKKLL").unwrap();
    let search = Search::new("", 0, 4).unwrap();
    covered!(
        search,
        "GET /v2/search",
        Operation::Search(search.clone()),
        Search,
        r#"{"hits":[],"offset":0,"limit":4,"total_hits":0}"#
    );
    let mut post = search;
    post.method = SearchMethod::Post;
    post.facets = vec![vec![Facet {
        field: FacetField::Versions,
        comparison: Comparison::Equal,
        value: "1.21.1".into(),
    }]];
    let request = covered!(
        search,
        "POST /v3/search",
        Operation::Search(post),
        ModernSearch,
        r#"{"hits":[],"page":1,"hits_per_page":4,"total_hits":0}"#
    );
    let body = post_body(&request);
    assert_eq!(body["limit"], "4");
    assert_eq!(body["facets"], r#"[["game_versions:1.21.1"]]"#);
    project_routes(&project, &projects).await;
    version_routes(project, projects, version).await;
}

async fn project_routes(project: &ProjectKey, projects: &Batch<ProjectKey>) {
    covered!(
        get_project,
        "GET /v2/project/demo",
        Operation::Project(project.clone()),
        Project,
        PROJECT
    );
    covered!(
        get_projects,
        "GET /v2/projects?ids=%5B%22demo%22%5D",
        Operation::Projects(projects.clone()),
        Projects,
        "[]"
    );
    covered!(
        random_projects,
        "GET /v2/projects_random?count=2",
        Operation::RandomProjects(2),
        Projects,
        format!("[{PROJECT}]")
    );
    covered!(
        check_project,
        "GET /v2/project/demo/check",
        Operation::CheckProject(project.clone()),
        ProjectCheck,
        r#"{"id":"AABBCCDD"}"#
    );
    covered!(
        project_dependencies,
        "GET /v2/project/demo/dependencies",
        Operation::Dependencies(project.clone()),
        Dependencies,
        r#"{"projects":[],"versions":[]}"#
    );
    covered!(
        project_disclosures,
        "GET /v3/project/demo/disclosures",
        Operation::Disclosures(project.clone()),
        Disclosures,
        r#"{"disclosures":[{"type":"telemetry","updated_at":"2026-11-28T00:00:00Z","consent":"opt_in","data_collected":["app version"]}]}"#
    );
    content_resolution(project).await;
}

async fn content_resolution(project: &ProjectKey) {
    let query = ContentRequest {
        project_id: project.clone(),
        version_id: None,
        content_type: ContentType::Mod,
        selected: ResolutionPreferences::default(),
        target: ResolutionPreferences {
            game_versions: vec!["1.21.1".into()],
            loaders: vec!["fabric".into()],
        },
        existing_project_ids: Vec::new(),
        update_channel: Some(VersionType::Release),
    };
    let request = covered!(
        resolve_content,
        "POST /v3/content/resolve",
        Operation::ResolveContent(query),
        Content,
        r#"{"primary":{"project_id":"AABBCCDD","version_id":"IIJJKKLL"},"dependencies":[],"skipped":[]}"#
    );
    assert_eq!(post_body(&request)["target"]["loaders"][0], "fabric");
}

async fn version_routes(project: ProjectKey, projects: Batch<ProjectKey>, version: VersionId) {
    let filters = VersionFilters {
        loaders: vec!["fabric".into()],
        game_versions: vec!["1.21.1".into()],
        version_type: Some(VersionType::Release),
        limit: Some(10),
        offset: Some(20),
        ..VersionFilters::default()
    };
    let query = ProjectVersions {
        project: project.clone(),
        filters: filters.clone(),
    };
    let request = covered!(
        list_versions,
        "GET /v2/project/demo/version",
        Operation::Versions(query),
        Versions,
        "[]"
    );
    assert!(request.contains("limit=10&offset=20&include_changelog=false"));
    assert!(request.contains("loaders=%5B%22fabric%22%5D"));
    covered!(
        get_version,
        "GET /v2/version/IIJJKKLL",
        Operation::Version(version.clone()),
        Version,
        VERSION
    );
    covered!(
        get_project_version,
        "GET /v2/project/demo/version/1.0+client",
        Operation::ProjectVersion(project, VersionKey::new("1.0+client").unwrap()),
        Version,
        VERSION
    );
    covered!(
        get_versions,
        "GET /v2/versions?ids=%5B%22IIJJKKLL%22%5D",
        Operation::BatchVersions(Batch::new(vec![version]).unwrap()),
        Versions,
        format!("[{VERSION}]")
    );
    covered!(
        find_versions,
        "GET /v2/versions?project_ids=%5B%22demo%22%5D",
        Operation::FindVersions(VersionSearch { projects, filters }),
        Versions,
        "[]"
    );
}

#[tokio::test]
async fn hash_routes() {
    let hash = HashLookup {
        algorithm: HashAlgorithm::Sha1,
        hash: HASH.to_ascii_uppercase(),
        version: None,
    };
    let filters = UpdateFilters {
        loaders: Some(vec!["fabric".into()]),
        game_versions: Some(vec!["1.21.1".into()]),
        version_types: None,
    };
    let batch = HashBatch {
        algorithm: HashAlgorithm::Sha1,
        hashes: Batch::new(vec![hash.hash.clone()]).unwrap(),
    };
    let version_path = format!("GET /v2/version_file/{HASH}?algorithm=sha1");
    covered!(
        version_by_hash,
        &version_path,
        Operation::FileVersion(hash.clone()),
        Version,
        VERSION
    );
    covered!(
        file_location,
        &format!("GET /v2/version_file/{HASH}/download"),
        Operation::FileDownload(hash.clone()),
        Download,
        redirect
    );
    let request = covered!(
        latest_version_by_hash,
        &format!("POST /v2/version_file/{HASH}/update"),
        Operation::FileUpdate(hash, filters.clone()),
        Version,
        VERSION
    );
    assert_eq!(post_body(&request)["loaders"][0], "fabric");
    hash_batches(batch, filters).await;
}

async fn hash_batches(batch: HashBatch, filters: UpdateFilters) {
    let request = covered!(
        projects_by_hash,
        "POST /v2/version_file/project",
        Operation::ProjectsByHash(batch.clone()),
        ProjectsByHash,
        format!(r#"{{"{HASH}":{PROJECT}}}"#)
    );
    assert_eq!(post_body(&request)["hashes"][0], HASH);
    covered!(
        versions_by_hash,
        "POST /v2/version_files",
        Operation::VersionsByHash(batch.clone()),
        VersionsByHash,
        format!(r#"{{"{HASH}":{VERSION}}}"#)
    );
    let query = BulkUpdate {
        hashes: batch,
        filters: filters.clone(),
    };
    let request = covered!(
        latest_versions_by_hash,
        "POST /v2/version_files/update",
        Operation::UpdatesByHash(query.clone()),
        VersionsByHash,
        "{}"
    );
    assert_eq!(post_body(&request)["game_versions"][0], "1.21.1");
    covered!(
        all_matching_updates,
        "POST /v2/version_files/update_many",
        Operation::AllUpdatesByHash(query),
        UpdatesByHash,
        format!(r#"{{"{HASH}":[{VERSION}]}}"#)
    );
    let query = IndividualUpdates {
        algorithm: HashAlgorithm::Sha1,
        hashes: Batch::new(vec![IndividualUpdate {
            hash: HASH.into(),
            filters,
        }])
        .unwrap(),
    };
    let request = covered!(
        individual_updates,
        "POST /v2/version_files/update_individual",
        Operation::IndividualUpdates(query),
        VersionsByHash,
        "{}"
    );
    assert_eq!(post_body(&request)["hashes"][0]["loaders"][0], "fabric");
}

#[tokio::test]
async fn rejects_redirects_outside_the_api_and_cdn() {
    let hash = HashLookup {
        algorithm: HashAlgorithm::Sha1,
        hash: HASH.into(),
        version: None,
    };
    let headers = "Location: https://example.invalid/file.jar\r\n";
    let (catalog, server) = mock(response("302 Found", headers, ""), Duration::ZERO, TIMEOUT);
    let (_, result) = catalog
        .lookup(Operation::FileDownload(hash))
        .await
        .into_parts();
    assert_eq!(result.unwrap_err(), CatalogError::InvalidResponse);
    server.join().unwrap();
}

fn ok(body: impl AsRef<str>) -> String {
    response("200 OK", "", body.as_ref())
}

fn moved() -> String {
    response(
        "307 Temporary Redirect",
        &format!("Location: {CDN}\r\n"),
        "",
    )
}

async fn verify(route: &str, operation: Operation, shape: &str, reply: String) -> String {
    let (catalog, server) = mock(reply, Duration::ZERO, TIMEOUT);
    let (cooldown, result) = catalog.lookup(operation).await.into_parts();
    let request = server.join().unwrap();
    assert_eq!(cooldown, None, "{route}");
    let payload = result.unwrap_or_else(|error| panic!("{route}: {error}"));
    assert!(
        serde_json::to_value(payload).unwrap().get(shape).is_some(),
        "{route}"
    );
    let suffix = request
        .strip_prefix(route)
        .unwrap_or_else(|| panic!("{route} != {request}"));
    assert!(suffix.starts_with([' ', '?', '&']), "{route} != {request}");
    assert!(
        !request.to_ascii_lowercase().contains("\r\nauthorization:"),
        "{route}"
    );
    request
}

fn post_body(request: &str) -> serde_json::Value {
    let (headers, body) = request.split_once("\r\n\r\n").unwrap();
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("content-type: application/json")
    );
    serde_json::from_str(body).unwrap()
}
