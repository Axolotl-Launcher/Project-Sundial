// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! HTTP and protocol clients.

use superlogical_spi::{FetchError, Network};

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
}

impl Network for ReqwestNetwork {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| FetchError::Retryable(error.to_string()))?;
        if let Some(error) = status_error(response.status()) {
            return Err(error);
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|error| FetchError::Retryable(error.to_string()))
    }
}

/// The error a non-success response stands for, or `None` when it succeeded.
fn status_error(status: reqwest::StatusCode) -> Option<FetchError> {
    if status.is_client_error() {
        return Some(FetchError::Fatal(format!("HTTP {status}")));
    }
    if status.is_success() {
        return None;
    }
    Some(FetchError::Retryable(format!("HTTP {status}")))
}

/// Whether `bytes` hashes to `expected_sha1`, compared without regard to hex case.
pub fn verify(bytes: &[u8], expected_sha1: &str) -> bool {
    let digest = sha1_smol::Sha1::from(bytes).digest();
    hex::encode(digest.bytes()).eq_ignore_ascii_case(expected_sha1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-1 of the empty input, as `sha1sum` reports it.
    const EMPTY_SHA1: &str = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
    /// SHA-1 of `abc`, as `sha1sum` reports it.
    const ABC_SHA1: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

    #[test]
    fn verify_accepts_a_matching_hash_in_either_case() {
        assert!(verify(b"", EMPTY_SHA1));
        assert!(verify(b"", &EMPTY_SHA1.to_uppercase()));
        assert!(verify(b"abc", ABC_SHA1));
        assert!(verify(b"abc", &ABC_SHA1.to_uppercase()));
    }

    #[test]
    fn verify_rejects_a_different_hash() {
        assert!(!verify(b"", ABC_SHA1));
        assert!(!verify(b"abc", EMPTY_SHA1));
        let zeros = "0".repeat(EMPTY_SHA1.len());
        assert!(!verify(b"", &zeros));
    }

    #[test]
    fn non_success_statuses_carry_the_retry_advice() {
        assert_eq!(
            status_error(reqwest::StatusCode::NOT_FOUND),
            Some(FetchError::Fatal("HTTP 404 Not Found".into()))
        );
        assert_eq!(
            status_error(reqwest::StatusCode::INTERNAL_SERVER_ERROR),
            Some(FetchError::Retryable(
                "HTTP 500 Internal Server Error".into()
            ))
        );
        assert_eq!(status_error(reqwest::StatusCode::OK), None);
    }
}
