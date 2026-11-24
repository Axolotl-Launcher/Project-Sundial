// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! HTTP and protocol clients.

use std::io::{self, Read};

use axlauncher_spi::{
    ByteRange, DownloadBody, DownloadProtocol, DownloadResponse, FetchError, Network,
};

pub mod auth;
mod dns;
mod transfer;

pub use dns::{DohConfig, DohUpstream};

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
    file_http1: reqwest::Client,
    file_http2: reqwest::Client,
}

impl ReqwestNetwork {
    /// A provider that sends every request through `client`.
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            file_http1: client.clone(),
            file_http2: client.clone(),
            client,
        }
    }

    /// The desktop client with bounded connection and request times.
    pub fn desktop() -> Result<Self, FetchError> {
        Self::with_doh(DohConfig::default())
    }

    /// Builds separate pools for metadata and file transfers with one DNS policy.
    pub fn with_doh(config: DohConfig) -> Result<Self, FetchError> {
        use std::sync::Arc;
        use std::time::Duration;

        let resolver = Arc::new(dns::DohResolver::new(config)?);
        let builder = || {
            reqwest::Client::builder()
                .dns_resolver(resolver.clone())
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
            file_http1: build(builder().http1_only())?,
            file_http2: build(builder().http2_prior_knowledge())?,
        })
    }
}

impl Network for ReqwestNetwork {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| FetchError::Retryable(error.to_string()))?;
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

    async fn open_download(
        &self,
        url: &str,
        protocol: DownloadProtocol,
        range: Option<ByteRange>,
    ) -> Result<DownloadResponse<impl DownloadBody>, FetchError> {
        let (client, version) = match protocol {
            DownloadProtocol::Automatic => (&self.client, None),
            DownloadProtocol::Http1 => (&self.file_http1, Some(reqwest::Version::HTTP_11)),
            DownloadProtocol::Http2 => (&self.file_http2, Some(reqwest::Version::HTTP_2)),
        };
        transfer::open(client, url, version, range).await
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
pub fn verify_reader(mut source: impl Read, expected_sha1: &str) -> io::Result<bool> {
    let mut hash = sha1_smol::Sha1::new();
    let mut buffer = [0; 8192];
    loop {
        match source.read(&mut buffer) {
            Ok(0) => return Ok(matches_sha1(hash, expected_sha1)),
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
