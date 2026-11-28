use axlauncher_network::metadata::{MetadataMethod, MetadataResponse};
use axlauncher_worker::modrinth::{CatalogError, Operation, Payload, Redirect};
use serde::{Serialize, de::DeserializeOwned};
use url::Url;

#[path = "routes_catalog.rs"]
mod catalog;
#[path = "routes_hash.rs"]
mod hashes;
#[path = "routes_people.rs"]
mod people;
#[path = "routes_public.rs"]
mod public;

type Decoder = fn(&[u8]) -> Result<Payload, CatalogError>;
type RedirectDecoder = fn(Redirect) -> Payload;
type Selection = Option<Result<Route, CatalogError>>;

pub(super) struct Route {
    url: Url,
    method: MetadataMethod,
    response: Format,
}

pub(super) enum Format {
    Json(Decoder),
    Redirect,
    Maven {
        accept: &'static str,
        decode_body: Decoder,
        decode_redirect: RedirectDecoder,
    },
}

pub(super) fn lookup(origin: &Url, operation: &Operation) -> Result<Route, CatalogError> {
    catalog::select(origin, operation)
        .or_else(|| hashes::select(origin, operation))
        .or_else(|| people::select(origin, operation))
        .or_else(|| public::select(origin, operation))
        .unwrap_or(Err(CatalogError::InvalidRequest))
}

impl Route {
    fn get(url: Url, decode: Decoder) -> Self {
        Self {
            url,
            method: MetadataMethod::Get,
            response: Format::Json(decode),
        }
    }

    fn post(url: Url, body: &impl Serialize, decode: Decoder) -> Result<Self, CatalogError> {
        let body = serde_json::to_vec(body).map_err(|_| CatalogError::InvalidRequest)?;
        Ok(Self {
            url,
            method: MetadataMethod::PostJson(body),
            response: Format::Json(decode),
        })
    }

    pub(super) fn into_parts(self) -> (Url, Format, MetadataMethod) {
        (self.url, self.response, self.method)
    }
}

impl Format {
    pub(super) fn accept(&self) -> &'static str {
        match self {
            Self::Json(_) => "application/json",
            Self::Redirect => "*/*",
            Self::Maven { accept, .. } => accept,
        }
    }

    pub(super) fn decode(
        &self,
        target: &Url,
        response: MetadataResponse,
    ) -> Result<Payload, CatalogError> {
        match (self, response) {
            (Self::Json(decode), MetadataResponse::Body(body))
            | (
                Self::Maven {
                    decode_body: decode,
                    ..
                },
                MetadataResponse::Body(body),
            ) => decode(&body),
            (Self::Json(_), MetadataResponse::Redirect { status, .. }) => {
                Err(CatalogError::Http { status })
            }
            (Self::Redirect, MetadataResponse::Body(_)) => Err(CatalogError::InvalidResponse),
            (Self::Redirect, MetadataResponse::Redirect { location, status }) => {
                redirect(target, &location, status).map(Payload::Download)
            }
            (
                Self::Maven {
                    decode_redirect, ..
                },
                MetadataResponse::Redirect { location, status },
            ) => redirect(target, &location, status).map(decode_redirect),
        }
    }
}

fn redirect(target: &Url, location: &str, status: u16) -> Result<Redirect, CatalogError> {
    let url = target
        .join(location)
        .map_err(|_| CatalogError::InvalidResponse)?;
    if !matches!(
        url.host_str(),
        Some("api.modrinth.com" | "cdn.modrinth.com")
    ) || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(CatalogError::InvalidResponse);
    }
    Ok(Redirect {
        status,
        location: url.into(),
    })
}

fn path(origin: &Url, segments: &[&str]) -> Url {
    let mut url = origin.clone();
    url.path_segments_mut()
        .expect("Modrinth API path")
        .clear()
        .extend(segments);
    url
}

fn json_string(input: &impl Serialize) -> Result<String, CatalogError> {
    serde_json::to_string(input).map_err(|_| CatalogError::InvalidRequest)
}

fn parameter(url: &mut Url, key: &str, value: impl std::fmt::Display) {
    url.query_pairs_mut().append_pair(key, &value.to_string());
}

fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, CatalogError> {
    serde_json::from_slice(body).map_err(|_| CatalogError::InvalidResponse)
}

macro_rules! get {
    ($origin:expr, [$($segments:expr),+], $payload:ident) => {
        $crate::catalog::routes::Route::get(
            $crate::catalog::routes::path($origin, &[$($segments),+]),
            |body| $crate::catalog::routes::parse(body).map(axlauncher_worker::modrinth::Payload::$payload),
        )
    };
}

use get;
