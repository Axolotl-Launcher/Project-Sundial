// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! HTTP and protocol clients.

use std::io::{self, Read};
use std::num::NonZeroUsize;
use std::sync::Arc;

use axlauncher_network_spi::{
    DownloadBody, DownloadMirror, DownloadProtocol, DownloadResponse, FetchError, FetchKind,
    Network, RangeRequest,
};
use url::Url;

pub mod auth;
mod dns;
pub mod metadata;
mod mirrors;
pub mod project_images;
mod transfer;

pub use dns::{DohConfig, DohUpstream};
pub use mirrors::{MirrorHosts, MirrorPreference, MirrorSettings};

/// A network provider that has not been given a transport yet.
///
/// Replacing this with a real HTTP client is the only change a caller sees.
#[derive(Debug, Default)]
pub struct UnimplementedNetwork;

impl Network for UnimplementedNetwork {
    async fn fetch(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Fatal("no transport configured".into()))
    }
}

/// An HTTP provider backed by a `reqwest::Client` supplied by the caller.
#[derive(Debug, Clone)]
pub struct ReqwestNetwork {
    client: reqwest::Client,
    file_auto: reqwest::Client,
    file_http1: reqwest::Client,
    file_http2: reqwest::Client,
    mirrors: Option<Arc<mirrors::Mirrors>>,
    request_policy: Option<fn(&Url) -> bool>,
    download_key: Option<DownloadKey>,
}

#[derive(Debug, Clone)]
struct DownloadKey {
    host: &'static str,
    value: reqwest::header::HeaderValue,
}

impl ReqwestNetwork {
    /// A provider that sends every request through `client`.
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            file_auto: client.clone(),
            file_http1: client.clone(),
            file_http2: client.clone(),
            client,
            mirrors: None,
            request_policy: None,
            download_key: None,
        }
    }

    /// The desktop client with bounded connection and request times.
    pub fn desktop() -> Result<Self, FetchError> {
        Self::with_doh(DohConfig::default())
    }

    /// The desktop client with the current download source preferences.
    pub fn desktop_with_mirrors(settings: MirrorSettings) -> Result<Self, FetchError> {
        Self::build(DohConfig::default(), settings, None, None)
    }

    /// Builds separate pools for metadata and file transfers with one DNS policy.
    pub fn with_doh(config: DohConfig) -> Result<Self, FetchError> {
        Self::build(config, MirrorSettings::default(), None, None)
    }

    /// A desktop client scoped to the caller's allowed origins, including redirects.
    pub fn desktop_with_download_policy(
        allow: fn(&Url) -> bool,
        key: Option<(&'static str, &axlauncher_network_spi::auth::ApiKey)>,
    ) -> Result<Self, FetchError> {
        let key = key
            .map(|(host, value)| {
                api_key_header(value.expose_secret()).map(|value| DownloadKey { host, value })
            })
            .transpose()?;
        Self::build(
            DohConfig::default(),
            MirrorSettings::default(),
            Some(allow),
            key,
        )
    }

    fn build(
        config: DohConfig,
        settings: MirrorSettings,
        allow: Option<fn(&Url) -> bool>,
        download_key: Option<DownloadKey>,
    ) -> Result<Self, FetchError> {
        use std::time::Duration;

        let resolver = Arc::new(dns::DohResolver::new(config)?);
        let user_agent = format!(
            "Axolotl-Launcher/{} ({})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        );
        let key_host = download_key.as_ref().map(|key| key.host);
        let builder = || {
            reqwest::Client::builder()
                .dns_resolver(resolver.clone())
                .user_agent(&user_agent)
                .redirect(redirect_policy(allow, key_host))
                .pool_idle_timeout(Duration::from_secs(15 * 60))
                .connect_timeout(Duration::from_secs(15))
        };
        let build = |builder: reqwest::ClientBuilder| {
            builder
                .build()
                .map_err(|error| FetchError::Fatal(error.to_string()))
        };
        Ok(Self {
            client: build(builder().timeout(Duration::from_secs(120)))?,
            file_auto: build(builder())?,
            file_http1: build(builder().http1_only())?,
            file_http2: build(builder().http2_prior_knowledge())?,
            mirrors: Some(Arc::new(mirrors::Mirrors::new(settings))),
            request_policy: allow,
            download_key,
        })
    }

    fn check_target(&self, url: &str) -> Result<(), FetchError> {
        if self
            .request_policy
            .is_some_and(|allow| !Url::parse(url).is_ok_and(|url| allow(&url)))
        {
            return Err(FetchError::Fatal(
                "request target is outside the download policy".into(),
            ));
        }
        Ok(())
    }

    fn alternate(&self, url: &str, kind: FetchKind) -> Option<DownloadMirror> {
        self.mirrors.as_ref()?.alternate(url, kind)
    }

    fn connect_failed(&self, url: &str) {
        if let Some(mirrors) = &self.mirrors {
            mirrors.connect_failed(url);
        }
    }

    fn fetch_request_error(&self, url: &str, error: reqwest::Error) -> FetchError {
        if error.is_connect() {
            self.connect_failed(url);
        }
        FetchError::Retryable(error.to_string())
    }

    fn download_key(&self, url: &str) -> Option<&reqwest::header::HeaderValue> {
        let key = self.download_key.as_ref()?;
        let url = Url::parse(url).ok()?;
        key_host_matches(&url, key.host).then_some(&key.value)
    }

    async fn fetch_exact(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.check_target(url)?;
        let _permit = match self.mirrors.as_ref().and_then(|mirrors| {
            let parsed = Url::parse(url).ok()?;
            mirrors.buffered_quota(parsed.host_str()?)
        }) {
            Some(quota) => Some(quota.acquire().await.expect("mirror quota is never closed")),
            None => None,
        };
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| self.fetch_request_error(url, error))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| FetchError::Retryable(error.to_string()))?
            .to_vec();

        if status.is_success() {
            return Ok(bytes);
        }
        Err(status_error(status, bytes))
    }
}

fn api_key_header(key: &str) -> Result<reqwest::header::HeaderValue, FetchError> {
    let mut value = reqwest::header::HeaderValue::from_str(key)
        .map_err(|_| FetchError::Fatal("invalid API key header value".into()))?;
    value.set_sensitive(true);
    Ok(value)
}

fn key_host_matches(url: &Url, host: &str) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some(host)
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

fn allows_key_redirect(host: &str, first: Option<&Url>, next: &Url) -> bool {
    !first.is_some_and(|url| key_host_matches(url, host)) || key_host_matches(next, host)
}

fn redirect_policy(
    allow: Option<fn(&Url) -> bool>,
    key_host: Option<&'static str>,
) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if allow.is_some_and(|allow| !allow(attempt.url())) {
            return attempt.error("redirect target is outside the download policy");
        }
        if key_host.is_some_and(|host| {
            !allows_key_redirect(host, attempt.previous().first(), attempt.url())
        }) {
            return attempt.error("refusing to redirect a keyed download outside its origin");
        }
        redirect(attempt)
    })
}

fn redirect(attempt: reqwest::redirect::Attempt<'_>) -> reqwest::redirect::Action {
    if attempt.url().scheme() != "https"
        && attempt.previous().iter().any(|url| url.scheme() == "https")
    {
        return attempt.error("refusing to downgrade an HTTPS request");
    }
    reqwest::redirect::Policy::default().redirect(attempt)
}

impl Network for ReqwestNetwork {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.fetch_with(url, FetchKind::Metadata, std::convert::identity)
            .await
    }

    async fn fetch_with<T, E>(
        &self,
        url: &str,
        kind: FetchKind,
        accept: impl Fn(Result<Vec<u8>, FetchError>) -> Result<T, E>,
    ) -> Result<T, E> {
        fetch_alternate(url, self.alternate(url, kind), async |source| {
            accept(self.fetch_exact(source).await)
        })
        .await
    }

    fn download_mirror(&self, url: &str, has_digest: bool) -> Option<DownloadMirror> {
        let kind = if has_digest {
            FetchKind::File
        } else {
            FetchKind::UnverifiedFile
        };
        self.alternate(url, kind)
    }

    fn download_connection_limit(&self, host: &str) -> NonZeroUsize {
        self.mirrors.as_ref().map_or(NonZeroUsize::MAX, |mirrors| {
            mirrors.download_connection_limit(host)
        })
    }

    async fn open_download(
        &self,
        url: &str,
        protocol: DownloadProtocol,
        range: Option<RangeRequest<'_>>,
    ) -> Result<DownloadResponse<impl DownloadBody>, FetchError> {
        self.check_target(url)?;
        let (client, version) = match protocol {
            DownloadProtocol::Automatic => (&self.file_auto, None),
            DownloadProtocol::Http1 => (&self.file_http1, Some(reqwest::Version::HTTP_11)),
            DownloadProtocol::Http2 => (&self.file_http2, Some(reqwest::Version::HTTP_2)),
        };
        transfer::open(client, url, version, range, self.download_key(url), || {
            self.connect_failed(url)
        })
        .await
        .map_err(FetchError::from)
    }
}

async fn fetch_alternate<T, E>(
    original: &str,
    mirror: Option<DownloadMirror>,
    fetch: impl AsyncFn(&str) -> Result<T, E>,
) -> Result<T, E> {
    let Some(mirror) = mirror else {
        return fetch(original).await;
    };
    if mirror.preferred {
        return match fetch(&mirror.url).await {
            Ok(value) => Ok(value),
            Err(_) => fetch(original).await,
        };
    }
    let original = fetch(original).await;
    match original {
        Ok(_) => original,
        Err(_) => fetch(&mirror.url).await.or(original),
    }
}

/// The error a non-success response stands for.
///
/// The body travels with a client error because an API can answer "this
/// version does not exist" as a 4xx whose body says so. A server error is
/// retryable and its body is a page, not an answer, so it is not kept.
fn status_error(status: reqwest::StatusCode, body: Vec<u8>) -> FetchError {
    if status.is_client_error() {
        return FetchError::Rejected {
            status: status.as_u16(),
            body,
        };
    }
    FetchError::Retryable(format!("HTTP {status}"))
}

/// Whether `bytes` hashes to `expected_sha1`, compared without regard to hex case.
pub fn verify(bytes: &[u8], expected_sha1: &str) -> bool {
    matches_sha1(sha1_smol::Sha1::from(bytes), expected_sha1)
}

/// Verifies a stream of bytes without retaining it in memory.
pub fn verify_reader(source: impl Read, expected_sha1: &str) -> io::Result<bool> {
    verify_reader_any(source, Some(expected_sha1), &[])
}

/// A stream matches if its digest equals any digest approved by the manifest.
pub fn verify_reader_any(
    mut source: impl Read,
    primary: Option<&str>,
    alternatives: &[String],
) -> io::Result<bool> {
    let mut hash = sha1_smol::Sha1::new();
    let mut buffer = [0; 8192];
    loop {
        match source.read(&mut buffer) {
            Ok(0) => {
                let actual = hex::encode(hash.digest().bytes());
                return Ok(primary
                    .into_iter()
                    .chain(alternatives.iter().map(String::as_str))
                    .any(|expected| actual.eq_ignore_ascii_case(expected)));
            }
            Ok(count) => hash.update(&buffer[..count]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

fn matches_sha1(hash: sha1_smol::Sha1, expected: &str) -> bool {
    hex::encode(hash.digest().bytes()).eq_ignore_ascii_case(expected)
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/network/src/lib.rs"]
mod tests;
