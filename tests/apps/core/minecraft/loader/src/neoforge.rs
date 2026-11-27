// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;

use super::*;
use axlauncher_spi::{FetchError, FetchKind, Network};

const MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const LEGACY_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/forge";

const METADATA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>net.neoforged</groupId>
  <artifactId>neoforge</artifactId>
  <versioning>
    <versions>
      <version>20.2.3-beta</version>
      <version>21.1.0</version>
      <version>20.2.93</version>
      <version>26.1.0.0-alpha.1</version>
      <version>26.1.0.0-alpha.1+snapshot-1</version>
      <version>26.1.0.0-alpha.9+snapshot-6</version>
      <version>26.1.0.0-alpha.10+snapshot-6</version>
      <version>26.1.0.0-alpha.15+pre-3</version>
      <version>26.1.0.2</version>
      <version>26.1.2.130</version>
      <version>21.0.167</version>
    </versions>
  </versioning>
</metadata>"#;

struct FakeNetwork {
    responses: BTreeMap<String, Result<Vec<u8>, FetchError>>,
}

impl Network for FakeNetwork {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.responses
            .get(url)
            .cloned()
            .unwrap_or_else(|| Err(FetchError::Fatal(format!("unexpected URL: {url}"))))
    }
}

struct StaleMirror;

impl Network for StaleMirror {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Fatal(format!("unclassified request for {url}")))
    }

    async fn fetch_with<T, E>(
        &self,
        url: &str,
        kind: FetchKind,
        accept: impl Fn(Result<Vec<u8>, FetchError>) -> Result<T, E>,
    ) -> Result<T, E> {
        assert_eq!(url, format!("{MAVEN}/maven-metadata.xml"));
        assert_eq!(kind, FetchKind::UnverifiedMetadata);
        let stale = b"<metadata><versioning><versions><version>20.2.3-beta</version></versions></versioning></metadata>";
        match accept(Ok(stale.to_vec())) {
            Ok(value) => Ok(value),
            Err(_) => accept(Ok(METADATA.as_bytes().to_vec())),
        }
    }
}

#[test]
fn a_requested_build_missing_from_the_mirror_is_looked_up_at_the_official_source() {
    let target = maven_metadata::target(LoaderKind::NeoForge, "1.20.2").unwrap();
    let coordinate = block_on(maven_metadata::resolve(
        &StaleMirror,
        &target,
        Some("20.2.93"),
    ))
    .unwrap();
    assert_eq!(coordinate, "20.2.93");
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

#[test]
fn a_game_version_selects_its_own_loader_versions() {
    let network = FakeNetwork {
        responses: BTreeMap::from([(
            format!("{MAVEN}/maven-metadata.xml"),
            Ok(METADATA.as_bytes().to_vec()),
        )]),
    };
    let api = NeoForgeApi::new(network);

    let versions = block_on(api.versions("1.20.2")).expect("versions");

    assert_eq!(
        versions
            .iter()
            .map(|v| v.version.as_str())
            .collect::<Vec<_>>(),
        ["20.2.93", "20.2.3-beta"]
    );
}

#[test]
fn a_game_version_without_builds_is_no_versions() {
    let network = FakeNetwork {
        responses: BTreeMap::from([(
            format!("{MAVEN}/maven-metadata.xml"),
            Ok(METADATA.as_bytes().to_vec()),
        )]),
    };
    let api = NeoForgeApi::new(network);

    let error = block_on(api.versions("1.5.2")).expect_err("none");

    assert!(matches!(error, LoaderError::NoVersions { .. }), "{error}");
}

#[test]
fn published_series_do_not_mix_patches_snapshots_and_base_releases() {
    let network = FakeNetwork {
        responses: BTreeMap::from([(
            format!("{MAVEN}/maven-metadata.xml"),
            Ok(METADATA.as_bytes().to_vec()),
        )]),
    };
    let api = NeoForgeApi::new(network);
    for (game, expected) in [
        ("1.21", &["21.0.167"][..]),
        ("26.1", &["26.1.0.2", "26.1.0.0-alpha.1"]),
        ("26.1.2", &["26.1.2.130"]),
        ("26.1-snapshot-1", &["26.1.0.0-alpha.1+snapshot-1"]),
        (
            "26.1-snapshot-6",
            &[
                "26.1.0.0-alpha.10+snapshot-6",
                "26.1.0.0-alpha.9+snapshot-6",
            ],
        ),
        ("26.1-pre-3", &["26.1.0.0-alpha.15+pre-3"]),
    ] {
        let listed = block_on(api.versions(game)).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|v| v.version.as_str())
                .collect::<Vec<_>>(),
            expected,
            "{game}"
        );
    }
}

#[test]
fn minecraft_1_20_1_only_uses_its_own_maven_repository() {
    let document = "<metadata><versioning><versions><version>1.20.1-47.1.5</version></versions></versioning></metadata>";
    let network = FakeNetwork {
        responses: BTreeMap::from([(
            format!("{LEGACY_MAVEN}/maven-metadata.xml"),
            Ok(document.as_bytes().to_vec()),
        )]),
    };
    let listed = block_on(NeoForgeApi::new(network).versions("1.20.1")).unwrap();
    assert_eq!(listed[0].version, "1.20.1-47.1.5");
}
