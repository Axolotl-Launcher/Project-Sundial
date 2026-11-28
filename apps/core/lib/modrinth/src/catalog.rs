use std::cell::OnceCell;

use axlauncher_network::metadata::{MetadataError, MetadataHttp};
use axlauncher_worker::modrinth::{CatalogError, Operation, Payload, Reply};
use serde::de::DeserializeOwned;
use url::Url;

const API_ORIGIN: &str = "https://api.modrinth.com/v2/";
const USER_AGENT: &str = concat!(
    "Axolotl-Launcher-Project-Sundial/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/Axolotl-Launcher/Project-Sundial)"
);

pub(super) struct Catalog {
    origin: Url,
    http: OnceCell<Result<MetadataHttp, MetadataError>>,
}

impl Catalog {
    pub(super) fn desktop() -> Self {
        Self {
            origin: Url::parse(API_ORIGIN).expect("Modrinth catalog origin"),
            http: OnceCell::new(),
        }
    }

    pub(super) async fn lookup(&self, operation: Operation) -> Reply {
        match self.fetch(&operation).await {
            Ok((delay, body)) => Reply::new(
                delay,
                body.map_err(|error| classify(error, delay))
                    .and_then(|body| decode(operation, &body)),
            ),
            Err(error) => Reply::new(None, Err(error)),
        }
    }

    async fn fetch(
        &self,
        operation: &Operation,
    ) -> Result<(Option<u32>, Result<Vec<u8>, MetadataError>), CatalogError> {
        operation.validate()?;
        let url = route(self.origin.clone(), operation);
        let http = self
            .http
            .get_or_init(|| MetadataHttp::desktop(USER_AGENT))
            .as_ref()
            .map_err(|error| classify(*error, None))?;
        http.get(url).await.map_err(|error| classify(error, None))
    }
}

fn route(mut url: Url, operation: &Operation) -> Url {
    {
        let mut path = url.path_segments_mut().expect("Modrinth catalog path");
        path.pop_if_empty();
        match operation {
            Operation::Search(_) => {
                path.extend(["search"]);
            }
            Operation::Project(key) => {
                path.extend(["project", key.as_str()]);
            }
            Operation::Versions(key) => {
                path.extend(["project", key.as_str(), "version"]);
            }
        }
    }
    match operation {
        Operation::Search(search) => {
            let (query, offset, limit) = search.parameters();
            url.query_pairs_mut()
                .append_pair("query", query)
                .append_pair("offset", &offset.to_string())
                .append_pair("limit", &limit.to_string());
        }
        Operation::Versions(_) => {
            url.query_pairs_mut()
                .append_pair("include_changelog", "false");
        }
        Operation::Project(_) => (),
    }
    url
}

fn classify(error: MetadataError, delay: Option<u32>) -> CatalogError {
    match error {
        MetadataError::Http(status @ (401 | 403)) => CatalogError::AccessDenied { status },
        MetadataError::Http(404) => CatalogError::NotFound,
        MetadataError::Http(429) => CatalogError::RateLimited {
            retry_after_seconds: delay.unwrap_or_default(),
        },
        MetadataError::Http(status) => CatalogError::Http { status },
        MetadataError::Timeout => CatalogError::Timeout,
        MetadataError::Transport => CatalogError::Transport,
        MetadataError::BodyTooLarge => CatalogError::InvalidResponse,
    }
}

fn decode(operation: Operation, bytes: &[u8]) -> Result<Payload, CatalogError> {
    match operation {
        Operation::Search(_) => parse(bytes).map(Payload::Search),
        Operation::Project(_) => parse(bytes).map(Payload::Project),
        Operation::Versions(_) => parse(bytes).map(Payload::Versions),
    }
}

fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CatalogError> {
    serde_json::from_slice(bytes).map_err(|_| CatalogError::InvalidResponse)
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/modrinth/src/catalog.rs"]
mod tests;
