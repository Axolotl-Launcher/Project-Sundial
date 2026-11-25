// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use std::cell::RefCell;

/// SHA-1 of the empty input, as `sha1sum` reports it.
const EMPTY_SHA1: &str = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
/// SHA-1 of `abc`, as `sha1sum` reports it.
const ABC_SHA1: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

#[test]
fn a_download_key_stays_on_its_https_origin() {
    let host = "edge.forgecdn.net";
    let client =
        ReqwestNetwork::desktop_with_download_policy(|_| true, Some((host, "fixture-key")))
            .unwrap();
    let edge = Url::parse("https://edge.forgecdn.net/files/example.jar").unwrap();
    let media = Url::parse("https://media.forgecdn.net/files/example.jar").unwrap();
    assert!(client.download_key(edge.as_str()).unwrap().is_sensitive());
    assert!(client.download_key(media.as_str()).is_none());
    assert!(
        client
            .download_key("http://edge.forgecdn.net/file")
            .is_none()
    );
    assert!(
        client
            .download_key("https://edge.forgecdn.net:8443/file")
            .is_none()
    );
    assert!(allows_key_redirect(host, Some(&edge), &edge));
    assert!(!allows_key_redirect(host, Some(&edge), &media));
    assert!(allows_key_redirect(host, Some(&media), &edge));
}

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
fn a_client_error_keeps_the_body_that_explains_it() {
    assert_eq!(
        status_error(reqwest::StatusCode::NOT_FOUND, b"no such version".to_vec()),
        FetchError::Rejected {
            status: 404,
            body: b"no such version".to_vec(),
        }
    );
    assert_eq!(
        status_error(reqwest::StatusCode::BAD_REQUEST, b"[]".to_vec()),
        FetchError::Rejected {
            status: 400,
            body: b"[]".to_vec(),
        }
    );
}

#[test]
fn a_server_error_is_retryable_and_does_not_keep_its_page() {
    assert_eq!(
        status_error(reqwest::StatusCode::INTERNAL_SERVER_ERROR, Vec::new()),
        FetchError::Retryable("HTTP 500 Internal Server Error".into())
    );
}

#[tokio::test]
async fn buffered_fallback_checks_content_and_keeps_the_upstream_error_on_total_failure() {
    let calls = RefCell::new(Vec::new());
    let mirror = |preferred| {
        Some(DownloadMirror {
            url: "mirror".into(),
            preferred,
        })
    };
    let result = fetch_alternate("upstream", mirror(true), async |source| {
        calls.borrow_mut().push(source.to_owned());
        (source == "upstream")
            .then_some(b"valid")
            .ok_or("bad digest")
    })
    .await;
    assert_eq!(result, Ok(b"valid"));
    assert_eq!(calls.take(), ["mirror", "upstream"]);

    let result = fetch_alternate("upstream", mirror(false), async |source| {
        calls.borrow_mut().push(source.to_owned());
        Err::<(), _>(source.to_owned())
    })
    .await;
    assert_eq!(result, Err("upstream".into()));
    assert_eq!(calls.take(), ["upstream", "mirror"]);

    let result = fetch_alternate("upstream", mirror(false), async |source| {
        calls.borrow_mut().push(source.to_owned());
        Ok::<_, ()>(())
    })
    .await;
    assert_eq!(result, Ok(()));
    assert_eq!(calls.into_inner(), ["upstream"]);
}
