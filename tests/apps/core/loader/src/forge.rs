// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use super::*;
use axlauncher_spi::FetchError;

const PROMOTIONS_BODY: &str = r#"{
  "homepage": "https://files.minecraftforge.net/",
  "promos": {
    "1.20.1-latest": "47.4.29",
    "1.20.1-recommended": "47.4.28",
    "1.19.2-recommended": "43.3.0",
    "1.20.1-latest-extra": "0"
  }
}"#;

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

fn installer() -> Vec<u8> {
    let install = r#"{
      "spec": 1,
      "version": "1.20.1-forge-47.4.28",
      "minecraft": "1.20.1",
      "json": "/version.json",
      "processors": [{ "jar": "net.minecraftforge:installertools:1.0", "classpath": [], "args": [] }],
      "data": {}
    }"#;
    let version = r#"{
      "id": "1.20.1-forge-47.4.28",
      "inheritsFrom": "1.20.1",
      "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
      "libraries": []
    }"#;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in [
        ("install_profile.json", install.as_bytes()),
        ("version.json", version.as_bytes()),
    ] {
        writer.start_file(name, options).expect("entry");
        writer.write_all(bytes).expect("write");
    }
    writer.finish().expect("zip").into_inner()
}

#[test]
fn the_two_promoted_versions_are_listed_newest_first() {
    let network = FakeNetwork {
        responses: BTreeMap::from([(PROMOTIONS.into(), Ok(PROMOTIONS_BODY.as_bytes().to_vec()))]),
    };
    let api = ForgeApi::new(network);

    let versions = block_on(api.versions("1.20.1")).expect("versions");

    assert_eq!(
        versions
            .iter()
            .map(|v| v.version.as_str())
            .collect::<Vec<_>>(),
        ["47.4.29", "47.4.28"]
    );
    assert_eq!(versions[0].stable, Some(false), "latest is not recommended");
    assert_eq!(versions[1].stable, Some(true), "recommended is stable");
}

#[test]
fn a_game_version_without_promotions_is_no_versions() {
    let network = FakeNetwork {
        responses: BTreeMap::from([(PROMOTIONS.into(), Ok(PROMOTIONS_BODY.as_bytes().to_vec()))]),
    };
    let api = ForgeApi::new(network);

    let error = block_on(api.versions("1.5.2")).expect_err("none");

    assert!(matches!(error, LoaderError::NoVersions { .. }), "{error}");
}

#[test]
fn the_profile_comes_from_the_installer_of_the_recommended_version() {
    let installer_url = ForgeApi::<FakeNetwork>::installer_url("1.20.1", "47.4.28");
    let network = FakeNetwork {
        responses: BTreeMap::from([
            (PROMOTIONS.into(), Ok(PROMOTIONS_BODY.as_bytes().to_vec())),
            (installer_url, Ok(installer())),
        ]),
    };
    let api = ForgeApi::new(network);

    let profile = block_on(api.profile(LoaderRequest {
        game_version: "1.20.1".into(),
        loader_version: None,
    }))
    .expect("profile");

    assert_eq!(profile.version_id, "1.20.1-forge-47.4.28");
}

#[test]
fn an_unpromoted_loader_version_is_refused() {
    let network = FakeNetwork {
        responses: BTreeMap::from([(PROMOTIONS.into(), Ok(PROMOTIONS_BODY.as_bytes().to_vec()))]),
    };
    let api = ForgeApi::new(network);

    let error = block_on(api.profile(LoaderRequest {
        game_version: "1.20.1".into(),
        loader_version: Some("1.0.0".into()),
    }))
    .expect_err("unknown");

    assert!(
        matches!(error, LoaderError::UnknownVersion { .. }),
        "{error}"
    );
}
