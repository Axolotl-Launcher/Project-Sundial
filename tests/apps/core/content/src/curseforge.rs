use std::cell::RefCell;
use std::collections::VecDeque;

use axlauncher_network_spi::auth::Response;
use futures::executor::block_on;
use serde_json::{Value, json};

use super::*;

struct Exchange {
    endpoint: String,
    body: Option<Value>,
    response: Result<Response, TransportError>,
}

struct MockHttp {
    key: ApiKey,
    exchanges: RefCell<VecDeque<Exchange>>,
}

impl AuthNetwork for MockHttp {
    async fn send(&self, request: Request<'_>) -> Result<Response, TransportError> {
        let expected = self
            .exchanges
            .borrow_mut()
            .pop_front()
            .expect("unexpected request");
        assert_eq!(request.url, format!("{API_ROOT}{}", expected.endpoint));
        assert!(request.api_key() == Some(&self.key));
        assert!(request.bearer.is_none());
        assert!(request.user_agent.starts_with("Axolotl-Launcher/"));
        if let Some(Body::Json(body)) = request.body {
            assert_eq!(serde_json::from_str::<Value>(&body).ok(), expected.body);
        } else {
            assert!(request.body.is_none() && expected.body.is_none());
        }
        expected.response
    }
}

fn catalog(exchanges: impl IntoIterator<Item = Exchange>) -> Catalog<MockHttp> {
    let key = ApiKey::new(&"a".repeat(30)).unwrap();
    Catalog::new(
        MockHttp {
            key: key.clone(),
            exchanges: RefCell::new(exchanges.into_iter().collect()),
        },
        key,
    )
}

fn get(endpoint: &str, data: Value) -> Exchange {
    reply(
        endpoint,
        Response::new(200, json!({"data": data}).to_string().into_bytes()),
    )
}

fn reply(endpoint: &str, response: Response) -> Exchange {
    Exchange {
        endpoint: endpoint.into(),
        body: None,
        response: Ok(response),
    }
}

fn page(endpoint: &str, data: Vec<Value>, index: u32, size: u8, total: u32) -> Exchange {
    reply(endpoint, Response::new(200, json!({
        "pagination": {"index": index, "pageSize": size, "resultCount": data.len(), "totalCount": total},
        "data": data
    }).to_string().into_bytes()))
}

fn project() -> Value {
    json!({"id": 24, "gameId": 432, "name": "Example", "slug": "example", "summary": "Sample",
        "classId": 6, "allowModDistribution": true, "links": {"websiteUrl": null}})
}

fn file() -> Value {
    json!({"id": 17, "modId": 24, "gameId": 432, "displayName": "Example", "fileName": "example.jar",
        "fileLength": 3, "isAvailable": true, "gameVersions": ["1.21"], "downloadUrl": null})
}

#[test]
fn search_and_listing_use_the_documented_query_keys_and_pagination() {
    let client = catalog([
        page(
            "/mods/search?gameId=432&searchFilter=a%26b+%2B+c&classId=6&index=2&pageSize=1",
            vec![project()],
            2,
            1,
            12_000,
        ),
        page("/mods/24/files?index=0&pageSize=3", vec![file()], 0, 3, 1),
        page(
            "/mods/search?gameId=432&index=9999&pageSize=1",
            vec![project()],
            9999,
            1,
            12_000,
        ),
    ]);
    block_on(async {
        let query = Search::new("a&b + c", PageRequest::new(2, 1).unwrap())
            .unwrap()
            .with_class(6)
            .unwrap();
        let result = client.search(query).await.unwrap();
        assert_eq!(result.items[0].slug, "example");
        assert_eq!(result.total_count(), 12_000);
        assert_eq!(result.next_request(), PageRequest::new(3, 1).ok());
        let result = client
            .list_files(24, PageRequest::new(0, 3).unwrap())
            .await
            .unwrap();
        assert_eq!(result.items[0].file_name, "example.jar");
        assert!(result.next_request().is_none());
        let query = Search::new("", PageRequest::new(9_999, 1).unwrap()).unwrap();
        assert!(client.search(query).await.unwrap().next_request().is_none());
    });
    assert!(client.network.exchanges.borrow().is_empty());
    for (index, size) in [(0, 0), (0, 51), (9_999, 2), (u32::MAX, 1)] {
        assert_eq!(
            PageRequest::new(index, size),
            Err(CatalogError::InvalidRequest)
        );
    }
}

#[test]
fn gets_mods_files_download_urls_html_and_categories() {
    let download = "https://edge.forgecdn.net/files/0/17/example.jar";
    let client = catalog([
        get("/mods/24", project()),
        get("/mods/24/files/17", file()),
        get("/mods/24/files/17/download-url", json!(download)),
        get("/mods/24/files/18/download-url", Value::Null),
        get("/mods/24/description", json!("<p>description</p>")),
        get("/mods/24/files/17/changelog", json!("<p>changelog</p>")),
        get(
            "/categories?gameId=432&classId=6",
            json!([
                {"id": 19, "gameId": 432, "name": "Technology", "slug": "technology",
                 "classId": 6, "parentCategoryId": 6, "isClass": false}
            ]),
        ),
    ]);
    block_on(async {
        assert_eq!(client.get_mod(24).await.unwrap().id, 24);
        assert_eq!(client.get_file(24, 17).await.unwrap().mod_id, 24);
        assert_eq!(
            client.download_url(24, 17).await.unwrap().unwrap().as_str(),
            download
        );
        assert!(client.download_url(24, 18).await.unwrap().is_none());
        assert_eq!(client.description(24).await.unwrap(), "<p>description</p>");
        assert_eq!(client.changelog(24, 17).await.unwrap(), "<p>changelog</p>");
        assert_eq!(
            client.categories(Some(6)).await.unwrap()[0].name,
            "Technology"
        );
    });
    assert!(client.network.exchanges.borrow().is_empty());
}

#[test]
fn importer_batches_mod_and_file_ids_at_bounded_sizes() {
    let mods: Vec<_> = (1..=11).collect();
    let files: Vec<_> = (1..=26).collect();
    let batch = |endpoint: &str, field: &str, ids: &[u32]| Exchange {
        body: Some(json!({field: ids})),
        ..get(endpoint, json!([]))
    };
    let client = catalog([
        batch("/mods", "modIds", &mods[..10]),
        batch("/mods", "modIds", &mods[10..]),
        batch("/mods/files", "fileIds", &files[..25]),
        batch("/mods/files", "fileIds", &files[25..]),
    ]);
    assert!(block_on(client.get_mods(&mods)).is_ok());
    assert!(block_on(client.get_files(&files)).is_ok());
    assert!(block_on(client.get_files(&[])).unwrap().is_empty());
    assert!(client.network.exchanges.borrow().is_empty());
}

#[test]
fn errors_are_typed_and_ignore_provider_error_text() {
    for (status, expected) in [
        (401, CatalogError::AccessDenied { status: 401 }),
        (403, CatalogError::AccessDenied { status: 403 }),
        (404, CatalogError::NotFound),
        (502, CatalogError::Http { status: 502 }),
        (200, CatalogError::InvalidResponse),
    ] {
        let client = catalog([reply(
            "/mods/24",
            Response::new(status, b"<not json>".to_vec()),
        )]);
        assert_eq!(block_on(client.get_mod(24)).unwrap_err(), expected);
    }
    let client = catalog([Exchange {
        endpoint: "/mods/24".into(),
        body: None,
        response: Err(TransportError::Timeout),
    }]);
    assert_eq!(
        block_on(client.get_mod(24)).unwrap_err(),
        CatalogError::Timeout
    );
    assert_eq!(
        block_on(client.get_file(24, 0)).unwrap_err(),
        CatalogError::InvalidRequest
    );
}

#[test]
fn rate_limits_stop_other_catalog_requests_without_sleeping_or_retrying() {
    for (header, expected) in [(None, 60), (Some(8), 8), (Some(u32::MAX), 3600)] {
        let mut response = Response::new(429, Vec::new());
        response.retry_after_seconds = header;
        let client = catalog([reply("/mods/24", response)]);
        assert_eq!(
            block_on(client.get_mod(24)).unwrap_err(),
            CatalogError::RateLimited {
                retry_after_seconds: expected
            },
        );
        assert!(matches!(
            block_on(client.categories(None)),
            Err(CatalogError::RateLimited { retry_after_seconds }) if (1..=expected).contains(&retry_after_seconds)
        ));
    }
}

#[test]
fn bad_pagination_and_non_https_downloads_are_rejected() {
    let client = catalog([
        page("/mods/24/files?index=0&pageSize=3", vec![file()], 1, 3, 1),
        get(
            "/mods/24/files/17/download-url",
            json!("http://invalid.test/example"),
        ),
    ]);
    let result = block_on(client.list_files(24, PageRequest::new(0, 3).unwrap()));
    assert_eq!(result.unwrap_err(), CatalogError::InvalidResponse);
    assert_eq!(
        block_on(client.download_url(24, 17)).unwrap_err(),
        CatalogError::InvalidResponse
    );
}
