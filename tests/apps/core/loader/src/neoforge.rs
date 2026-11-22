// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;

use super::*;
use axlauncher_spi::{FetchError, Network};

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
fn the_metadata_document_yields_its_versions() {
    assert_eq!(
        versions_in(METADATA),
        ["20.2.3-beta", "21.1.0", "20.2.93", "26.1.0.0-alpha.1"]
    );
}

#[test]
fn a_game_version_drops_its_leading_one() {
    assert_eq!(game_prefix("1.20.2"), "20.2");
    assert_eq!(game_prefix("26.1"), "26.1");
}
