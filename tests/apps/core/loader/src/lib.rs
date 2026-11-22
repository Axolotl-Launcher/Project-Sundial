// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;

use super::*;
use axlauncher_spi::{FetchError, Network};

/// The listing Fabric returns for a supported version, trimmed to the fields
/// this provider reads. The shape is copied from a real response.
const FABRIC_LISTING: &str = r#"[
  {
    "loader": {
      "separator": ".",
      "build": 1,
      "maven": "net.fabricmc:fabric-loader:0.20.1",
      "version": "0.20.1",
      "stable": true
    },
    "intermediary": { "maven": "net.fabricmc:intermediary:1.21.11", "version": "1.21.11", "stable": true }
  },
  {
    "loader": {
      "separator": ".",
      "build": 9,
      "maven": "net.fabricmc:fabric-loader:0.20.0-beta.9",
      "version": "0.20.0-beta.9",
      "stable": false
    },
    "intermediary": { "maven": "net.fabricmc:intermediary:1.21.11", "version": "1.21.11", "stable": true }
  }
]"#;

/// The listing Quilt returns: the same shape without a `stable` field.
const QUILT_LISTING: &str = r#"[
  {
    "loader": {
      "maven": "org.quiltmc:quilt-loader:0.20.0-beta.9",
      "version": "0.20.0-beta.9",
      "build": 9,
      "separator": "."
    },
    "hashed": { "maven": "org.quiltmc:hashed:1.21.11", "version": "1.21.11" }
  }
]"#;

/// The profile Fabric returns for 0.20.1, trimmed to the fields the port
/// validates. A real one also carries libraries and arguments, which travel
/// through untouched.
const FABRIC_PROFILE: &str = r#"{
  "id": "fabric-loader-0.20.1-1.21.11",
  "inheritsFrom": "1.21.11",
  "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
  "arguments": { "game": [], "jvm": [] },
  "libraries": [],
  "type": "release",
  "time": "2024-01-01T00:00:00+00:00",
  "releaseTime": "2024-01-01T00:00:00+00:00"
}"#;

const FABRIC_VERSIONS_URL: &str = "https://meta.fabricmc.net/v2/versions/loader/1.21.11";
const FABRIC_PROFILE_URL: &str =
    "https://meta.fabricmc.net/v2/versions/loader/1.21.11/0.20.1/profile/json";
const QUILT_VERSIONS_URL: &str = "https://meta.quiltmc.org/v3/versions/loader/1.21.11";

/// Answers a fixed body per URL, and records what was asked for.
#[derive(Default)]
struct FakeNetwork {
    responses: BTreeMap<String, Result<Vec<u8>, FetchError>>,
    asked: std::sync::Mutex<Vec<String>>,
}

impl FakeNetwork {
    fn with(mut self, url: &str, body: &str) -> Self {
        self.responses.insert(url.into(), Ok(body.into()));
        self
    }

    fn failing(mut self, url: &str, error: FetchError) -> Self {
        self.responses.insert(url.into(), Err(error));
        self
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("asked").clone()
    }
}

impl Network for FakeNetwork {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.asked.lock().expect("asked").push(url.to_owned());
        self.responses
            .get(url)
            .cloned()
            .unwrap_or_else(|| Err(FetchError::Fatal(format!("unexpected URL: {url}"))))
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};

    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = std::pin::pin!(future);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

fn request(loader_version: Option<&str>) -> LoaderRequest {
    LoaderRequest {
        game_version: "1.21.11".into(),
        loader_version: loader_version.map(str::to_string),
    }
}

#[test]
fn a_listing_becomes_loader_versions() {
    let network = FakeNetwork::default().with(FABRIC_VERSIONS_URL, FABRIC_LISTING);
    let api = LoaderApi::fabric(network);

    let versions = block_on(api.versions("1.21.11")).expect("versions");

    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].version, "0.20.1");
    assert_eq!(versions[0].stable, Some(true));
    assert_eq!(versions[1].stable, Some(false));
}

#[test]
fn a_loader_that_does_not_state_stability_reports_it_as_unknown() {
    let network = FakeNetwork::default().with(QUILT_VERSIONS_URL, QUILT_LISTING);
    let api = LoaderApi::quilt(network);

    let versions = block_on(api.versions("1.21.11")).expect("versions");

    assert_eq!(versions[0].stable, None);
}

#[test]
fn an_unsupported_game_version_is_no_versions_rather_than_a_failure() {
    // Fabric answers 400 with an empty list for a version it has no builds for.
    let network = FakeNetwork::default().failing(
        "https://meta.fabricmc.net/v2/versions/loader/1.5.2",
        FetchError::Rejected {
            status: 400,
            body: b"[]".to_vec(),
        },
    );
    let api = LoaderApi::fabric(network);

    let error = block_on(api.versions("1.5.2")).expect_err("no versions");

    assert!(matches!(error, LoaderError::NoVersions { .. }), "{error}");
    assert!(error.to_string().contains("1.5.2"), "{error}");
}

#[test]
fn an_empty_successful_listing_is_also_no_versions() {
    let network = FakeNetwork::default().with(FABRIC_VERSIONS_URL, "[]");
    let api = LoaderApi::fabric(network);

    let error = block_on(api.versions("1.21.11")).expect_err("no versions");

    assert!(matches!(error, LoaderError::NoVersions { .. }), "{error}");
}

#[test]
fn an_unreachable_service_names_the_loader_and_keeps_the_cause() {
    let network = FakeNetwork::default().failing(
        FABRIC_VERSIONS_URL,
        FetchError::Retryable("timed out".into()),
    );
    let api = LoaderApi::fabric(network);

    let error = block_on(api.versions("1.21.11")).expect_err("unreachable");

    assert!(matches!(error, LoaderError::Unavailable { .. }), "{error}");
    assert!(error.to_string().contains("fabric"), "{error}");
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn a_body_that_is_not_a_listing_is_malformed() {
    let network = FakeNetwork::default().with(FABRIC_VERSIONS_URL, "<html>nope</html>");
    let api = LoaderApi::fabric(network);

    let error = block_on(api.versions("1.21.11")).expect_err("malformed");

    assert!(matches!(error, LoaderError::Malformed { .. }), "{error}");
}

#[test]
fn a_profile_for_the_named_version_is_returned_verbatim() {
    let network = FakeNetwork::default()
        .with(FABRIC_VERSIONS_URL, FABRIC_LISTING)
        .with(FABRIC_PROFILE_URL, FABRIC_PROFILE);
    let api = LoaderApi::fabric(network);

    let profile = block_on(api.profile(request(Some("0.20.1")))).expect("profile");

    assert_eq!(profile.version_id, "fabric-loader-0.20.1-1.21.11");
    assert_eq!(profile.document, FABRIC_PROFILE.as_bytes());
    assert!(profile.files.is_empty());
}

#[test]
fn without_a_named_version_the_recommended_one_is_used() {
    let network = FakeNetwork::default()
        .with(FABRIC_VERSIONS_URL, FABRIC_LISTING)
        .with(FABRIC_PROFILE_URL, FABRIC_PROFILE);
    let api = LoaderApi::fabric(network);

    let profile = block_on(api.profile(request(None))).expect("profile");

    // 0.20.1 is the stable entry; the beta listed after it is not chosen.
    assert_eq!(profile.version_id, "fabric-loader-0.20.1-1.21.11");
}

#[test]
fn a_loader_that_states_no_stability_uses_its_first_entry() {
    let profile_url =
        "https://meta.quiltmc.org/v3/versions/loader/1.21.11/0.20.0-beta.9/profile/json";
    let quilt_profile = FABRIC_PROFILE.replace(
        "fabric-loader-0.20.1-1.21.11",
        "quilt-loader-0.20.0-beta.9-1.21.11",
    );
    let network = FakeNetwork::default()
        .with(QUILT_VERSIONS_URL, QUILT_LISTING)
        .with(profile_url, &quilt_profile);
    let api = LoaderApi::quilt(network);

    let profile = block_on(api.profile(request(None))).expect("profile");

    assert_eq!(profile.version_id, "quilt-loader-0.20.0-beta.9-1.21.11");
}

#[test]
fn a_version_the_provider_does_not_offer_is_named_rather_than_fetched() {
    let network = FakeNetwork::default().with(FABRIC_VERSIONS_URL, FABRIC_LISTING);
    let api = LoaderApi::fabric(network);

    let error = block_on(api.profile(request(Some("0.99.0")))).expect_err("unknown");

    assert!(
        matches!(error, LoaderError::UnknownVersion { ref version, .. } if version == "0.99.0"),
        "{error}"
    );
    assert_eq!(api.network.asked(), vec![FABRIC_VERSIONS_URL]);
}

#[test]
fn a_profile_that_extends_another_game_version_is_malformed() {
    let mismatched = FABRIC_PROFILE.replace(
        r#""inheritsFrom": "1.21.11""#,
        r#""inheritsFrom": "1.20.1""#,
    );
    let network = FakeNetwork::default()
        .with(FABRIC_VERSIONS_URL, FABRIC_LISTING)
        .with(FABRIC_PROFILE_URL, &mismatched);
    let api = LoaderApi::fabric(network);

    let error = block_on(api.profile(request(Some("0.20.1")))).expect_err("mismatch");

    assert!(matches!(error, LoaderError::Malformed { .. }), "{error}");
}

#[test]
fn the_quilt_provider_asks_quilt() {
    let network = FakeNetwork::default().with(QUILT_VERSIONS_URL, QUILT_LISTING);
    let api = LoaderApi::quilt(network);

    assert_eq!(api.kind(), LoaderKind::Quilt);
    block_on(api.versions("1.21.11")).expect("versions");
    assert_eq!(api.network.asked(), vec![QUILT_VERSIONS_URL]);
}

#[test]
fn the_fabric_provider_names_itself() {
    let api = LoaderApi::fabric(FakeNetwork::default());

    assert_eq!(api.kind(), LoaderKind::Fabric);
}
