// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

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
