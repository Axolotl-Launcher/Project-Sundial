// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! HTTP and protocol clients.

use axlauncher_spi::{FetchError, Network};

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
}

impl ReqwestNetwork {
    /// A provider that sends every request through `client`.
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// The desktop client with bounded connection and request times.
    pub fn desktop() -> Result<Self, FetchError> {
        use std::time::Duration;

        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .build()
            .map(Self::new)
            .map_err(|error| FetchError::Fatal(error.to_string()))
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
    let digest = sha1_smol::Sha1::from(bytes).digest();
    hex::encode(digest.bytes()).eq_ignore_ascii_case(expected_sha1)
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/network/src/lib.rs"]
mod tests;
