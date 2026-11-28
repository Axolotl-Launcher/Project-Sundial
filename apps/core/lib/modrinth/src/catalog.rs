use std::cell::OnceCell;

use axlauncher_network::metadata::{MetadataError, MetadataHttp};
use axlauncher_worker::modrinth::{CatalogError, Operation, Reply};
use url::Url;

#[path = "routes.rs"]
mod routes;

const API_ORIGIN: &str = "https://api.modrinth.com/";
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
            Ok(reply) => reply,
            Err(error) => Reply::new(None, Err(error)),
        }
    }

    async fn fetch(&self, operation: &Operation) -> Result<Reply, CatalogError> {
        operation.validate()?;
        let request = routes::lookup(&self.origin, operation)?;
        let http = self
            .http
            .get_or_init(|| MetadataHttp::desktop(USER_AGENT))
            .as_ref()
            .map_err(|error| classify(*error, None))?;
        let (target, format, method) = request.into_parts();
        let (delay, result) = http
            .send(method, target.clone(), format.accept())
            .await
            .map_err(|error| classify(error, None))?;
        Ok(Reply::new(
            delay,
            result
                .map_err(|error| classify(error, delay))
                .and_then(|response| format.decode(&target, response)),
        ))
    }
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

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/modrinth/src/catalog.rs"]
mod tests;
