//! Transport contracts for metadata, incremental downloads and authentication.

#![forbid(unsafe_code)]

use std::time::Duration;

pub mod auth;
mod sources;
mod transfer;

pub use sources::{DownloadMirror, FetchKind, ServiceHost};
pub use transfer::{
    ByteRange, DownloadBody, DownloadEtag, DownloadProtocol, DownloadRepresentation,
    DownloadResponse, RangeRequest, ResponseRange,
};

/// Fetches bytes over the network.
pub trait Network {
    /// Resolves a URL's body.
    fn fetch(&self, url: &str) -> impl Future<Output = Result<Vec<u8>, FetchError>>;

    /// Fetches a resource and lets the caller accept or reject each candidate response.
    fn fetch_with<T, E>(
        &self,
        url: &str,
        _kind: FetchKind,
        accept: impl Fn(Result<Vec<u8>, FetchError>) -> Result<T, E>,
    ) -> impl Future<Output = Result<T, E>> {
        async move { accept(self.fetch(url).await) }
    }

    /// Supplies a mirror for a file transfer with the given integrity guarantee.
    fn download_mirror(&self, _url: &str, _has_digest: bool) -> Option<DownloadMirror> {
        None
    }

    /// Caps active file requests to a host, including requests using byte ranges.
    fn download_connection_limit(&self, _host: &str) -> std::num::NonZeroUsize {
        std::num::NonZeroUsize::MAX
    }

    /// Opens a file transfer. Providers that only fetch bodies may return a full response.
    fn open_download(
        &self,
        url: &str,
        _protocol: DownloadProtocol,
        _range: Option<RangeRequest<'_>>,
    ) -> impl Future<Output = Result<DownloadResponse<impl DownloadBody>, FetchError>> {
        async move {
            let body = bytes::Bytes::from(self.fetch(url).await?);
            Ok(DownloadResponse {
                length: Some(body.len() as u64),
                range: None,
                representation: None,
                body: Some(body),
            })
        }
    }
}

/// Why a fetch failed, reduced to what a caller can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// Worth retrying: a timeout, a 5xx, a dropped connection.
    Retryable(String),
    /// A temporary HTTP failure with an optional server-supplied minimum delay.
    HttpRetryable {
        status: u16,
        retry_after: Option<Duration>,
    },
    /// Not worth retrying: a malformed URL, no transport configured.
    Fatal(String),
    /// The server rejected the request and may have supplied an explanation.
    ///
    /// The body is kept because an API can put meaning in it: a version that
    /// does not exist and a request the server rejected can both arrive as a
    /// 4xx, and only the body tells them apart. Dropping it would leave a
    /// caller unable to say which happened.
    Rejected {
        /// The HTTP status the server answered with.
        status: u16,
        body: Vec<u8>,
    },
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retryable(reason) => write!(formatter, "retryable: {reason}"),
            Self::HttpRetryable { status, .. } => write!(formatter, "HTTP {status}"),
            Self::Fatal(reason) => formatter.write_str(reason),
            Self::Rejected { status, body } if body.is_empty() => {
                write!(formatter, "HTTP {status}")
            }
            Self::Rejected { status, body } => {
                write!(formatter, "HTTP {status}: {}", body_excerpt(body))
            }
        }
    }
}

impl std::error::Error for FetchError {}

const BODY_EXCERPT_LEN: usize = 200;

fn body_excerpt(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let trimmed = text.trim();
    let mut excerpt: String = trimmed.chars().take(BODY_EXCERPT_LEN).collect();
    if trimmed.chars().count() > BODY_EXCERPT_LEN {
        excerpt.push('…');
    }
    excerpt
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/spi/network/src/lib.rs"]
mod tests;
