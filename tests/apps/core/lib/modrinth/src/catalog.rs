use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use axlauncher_worker::modrinth::{ProjectKey, Search, VersionType};

use super::{Catalog, CatalogError, MetadataHttp, Operation, Payload, USER_AGENT, Url};

const TIMEOUT: Duration = Duration::from_secs(3);

#[tokio::test]
async fn searches_with_encoded_parameters_and_returns_typed_public_hits() {
    let body = r#"{
        "hits": [{
            "project_id": "AABBCCDD", "slug": "my-mod", "title": "My Mod",
            "description": "Build things", "project_type": "mod", "icon_url": null,
            "downloads": 74, "access_token": "not-part-of-the-catalog"
        }],
        "offset": 20, "limit": 10, "total_hits": 42
    }"#;
    let (catalog, server) = mock(response("200 OK", "", body), Duration::ZERO, TIMEOUT);
    let reply = catalog
        .lookup(Operation::Search(
            Search::new("snow + ice & 雪", 20, 10).unwrap(),
        ))
        .await;
    assert!(
        !serde_json::to_string(&reply)
            .unwrap()
            .contains("not-part-of-the-catalog")
    );
    let Payload::Search(page) = reply.into_parts().1.unwrap() else {
        panic!("expected a search page");
    };
    assert_eq!((page.offset, page.limit, page.total_hits), (20, 10, 42));
    assert_eq!(page.hits[0].project_id, "AABBCCDD");
    let request = server.join().unwrap();
    assert!(
        request.starts_with("GET /v2/search?query=snow+%2B+ice+%26+%E9%9B%AA&offset=20&limit=10 ")
    );
    let headers = request.to_ascii_lowercase();
    assert!(headers.contains(&format!("user-agent: {}", USER_AGENT.to_ascii_lowercase())));
    assert!(headers.contains("accept: application/json"));
    assert!(!headers.contains("authorization:"));
    assert!(!headers.contains("cookie:"));
}

#[tokio::test]
async fn requests_project_details_by_slug_and_versions_by_id() {
    let body = r#"{
        "id": "AABBCCDD", "slug": "my-mod", "title": "My Mod", "description": "Build things",
        "body": "More details", "project_type": "mod", "icon_url": null, "downloads": 74
    }"#;
    let (catalog, server) = mock(response("200 OK", "", body), Duration::ZERO, TIMEOUT);
    let Payload::Project(project) = catalog
        .lookup(Operation::Project(ProjectKey::new("my-mod").unwrap()))
        .await
        .into_parts()
        .1
        .unwrap()
    else {
        panic!("expected project details");
    };
    assert_eq!(project.id, "AABBCCDD");
    assert_eq!(project.body, "More details");
    assert!(
        server
            .join()
            .unwrap()
            .starts_with("GET /v2/project/my-mod ")
    );

    let body = r#"[{
        "id": "IIJJKKLL", "project_id": "AABBCCDD", "name": "Early build", "version_number": "1.0.0",
        "date_published": "2026-11-01T00:00:00Z", "version_type": "beta", "game_versions": ["1.21.1"],
        "loaders": ["fabric"], "files": [{
            "url": "https://cdn.modrinth.com/data/AABBCCDD/versions/IIJJKKLL/mod.jar",
            "filename": "mod.jar", "size": 42, "primary": true, "hashes": {"sha512": "0123"}
        }]
    }]"#;
    let (catalog, server) = mock(response("200 OK", "", body), Duration::ZERO, TIMEOUT);
    let Payload::Versions(versions) = catalog
        .lookup(Operation::Versions(ProjectKey::new("AABBCCDD").unwrap()))
        .await
        .into_parts()
        .1
        .unwrap()
    else {
        panic!("expected project versions");
    };
    assert!(matches!(versions[0].version_type, VersionType::Beta));
    assert_eq!(versions[0].files[0].filename, "mod.jar");
    assert!(
        server
            .join()
            .unwrap()
            .starts_with("GET /v2/project/AABBCCDD/version?include_changelog=false ")
    );
}

#[tokio::test]
async fn maps_http_and_parse_failures_without_returning_response_text() {
    for (status, expected) in [
        ("404 Not Found", CatalogError::NotFound),
        (
            "401 Unauthorized",
            CatalogError::AccessDenied { status: 401 },
        ),
        ("403 Forbidden", CatalogError::AccessDenied { status: 403 }),
        ("410 Gone", CatalogError::Http { status: 410 }),
        (
            "503 Service Unavailable",
            CatalogError::Http { status: 503 },
        ),
        ("200 OK", CatalogError::InvalidResponse),
    ] {
        let (delay, result) =
            project_reply(response(status, "", "not JSON"), Duration::ZERO, TIMEOUT).await;
        assert_eq!(delay, None);
        assert_eq!(result.unwrap_err(), expected);
    }
}

#[tokio::test]
async fn backs_off_on_429_with_or_without_server_hints() {
    let cases = [
        ("X-Ratelimit-Reset: 4\r\nRetry-After: 12\r\n", 12),
        ("", 60),
    ];
    for (headers, seconds) in cases {
        let (delay, result) = versions_reply("429 Too Many Requests", headers).await;
        assert_eq!(delay, Some(seconds));
        assert_eq!(
            result.unwrap_err(),
            CatalogError::RateLimited {
                retry_after_seconds: seconds
            }
        );
    }
}

#[tokio::test]
async fn reports_an_exhausted_rate_window_on_success() {
    let headers = "X-Ratelimit-Remaining: 0\r\nX-Ratelimit-Reset: 8\r\n";
    let (delay, result) = versions_reply("200 OK", headers).await;
    assert_eq!(delay, Some(8));
    assert!(matches!(result, Ok(Payload::Versions(versions)) if versions.is_empty()));
}

#[tokio::test]
async fn bounds_response_size_and_time() {
    let oversize = "HTTP/1.1 200 OK\r\nContent-Length: 10000000\r\nConnection: close\r\n\r\n";
    let (_, result) = project_reply(oversize.to_owned(), Duration::ZERO, TIMEOUT).await;
    assert_eq!(result.unwrap_err(), CatalogError::InvalidResponse);

    let (_, result) = project_reply(
        response("200 OK", "", "{}"),
        Duration::from_millis(800),
        Duration::from_millis(350),
    )
    .await;
    assert_eq!(result.unwrap_err(), CatalogError::Timeout);
}

async fn project_reply(
    response: String,
    delay: Duration,
    timeout: Duration,
) -> (Option<u32>, Result<Payload, CatalogError>) {
    let (catalog, server) = mock(response, delay, timeout);
    let reply = catalog
        .lookup(Operation::Project(ProjectKey::new("my-mod").unwrap()))
        .await;
    server.join().unwrap();
    reply.into_parts()
}

async fn versions_reply(
    status: &str,
    headers: &str,
) -> (Option<u32>, Result<Payload, CatalogError>) {
    let (catalog, server) = mock(response(status, headers, "[]"), Duration::ZERO, TIMEOUT);
    let reply = catalog
        .lookup(Operation::Versions(ProjectKey::new("my-mod").unwrap()))
        .await;
    server.join().unwrap();
    reply.into_parts()
}

fn response(status: &str, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
        body.len(),
    )
}

fn mock(response: String, delay: Duration, timeout: Duration) -> (Catalog, JoinHandle<String>) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let catalog = Catalog {
        origin: Url::parse(&format!("http://{}/v2/", listener.local_addr().unwrap())).unwrap(),
        http: std::cell::OnceCell::new(),
    };
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .user_agent(USER_AGENT)
        .timeout(timeout)
        .build()
        .unwrap();
    assert!(catalog.http.set(Ok(MetadataHttp::new(http))).is_ok());
    let server = thread::spawn(move || {
        let mut stream = accept(&listener);
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        let request = read_request(&stream);
        thread::sleep(delay);
        let _ = stream.write_all(response.as_bytes());
        request
    });
    (catalog, server)
}

fn accept(listener: &TcpListener) -> TcpStream {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("accepting a Modrinth request: {error}"),
        }
    }
}

fn read_request(stream: &TcpStream) -> String {
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            return request;
        }
        request.push_str(&line);
    }
}
