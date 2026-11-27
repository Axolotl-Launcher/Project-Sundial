// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use super::*;
use axlauncher_network_spi::FetchError;

const MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
const PROMOTIONS_BODY: &str = r#"{
  "homepage": "https://files.minecraftforge.net/",
  "promos": {
    "1.20.1-latest": "47.4.29",
    "1.20.1-recommended": "47.4.28",
    "1.19.2-recommended": "43.3.0",
    "1.20.1-latest-extra": "0"
  }
}"#;

const METADATA: &str = r#"<metadata><versioning><versions>
    <version>1.20.1-47.4.0</version>
    <version>1.20.1-47.4.29</version>
    <version>1.7.10-10.13.4.1614</version>
    <version>1.7.10-10.13.4.1614-1.7.10</version>
    <version>1.10-12.18.0.2000-1.10.0</version>
    <version>1.20.1-47.4.28</version>
    <version>1.19.2-43.3.0</version>
    <version>26.1-snapshot-1-62.0.1</version>
    <version>26.1-62.0.2</version>
</versions></versioning></metadata>"#;

fn catalog() -> FakeNetwork {
    FakeNetwork {
        responses: BTreeMap::from([(
            format!("{MAVEN}/maven-metadata.xml"),
            Ok(METADATA.as_bytes().to_vec()),
        )]),
        ..FakeNetwork::default()
    }
}

#[derive(Default)]
struct FakeNetwork {
    responses: BTreeMap<String, Result<Vec<u8>, FetchError>>,
    metadata_requests: Cell<usize>,
}

impl Network for FakeNetwork {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        if url == format!("{MAVEN}/maven-metadata.xml") {
            self.metadata_requests.set(self.metadata_requests.get() + 1);
        }
        self.responses
            .get(url)
            .cloned()
            .unwrap_or_else(|| Err(FetchError::Fatal(format!("unexpected URL: {url}"))))
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(future)
}

fn installer(game: &str, id: &str) -> Vec<u8> {
    let install = format!(
        r#"{{
      "spec": 1,
      "version": "{id}",
      "minecraft": "{game}",
      "json": "/version.json",
      "processors": [{{ "jar": "net.minecraftforge:installertools:1.0", "classpath": [], "args": [] }}],
      "data": {{}}
    }}"#
    );
    let version = format!(
        r#"{{
      "id": "{id}",
      "inheritsFrom": "{game}",
      "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
      "libraries": []
    }}"#
    );
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
fn the_catalog_includes_unpromoted_builds_newest_first() {
    let api = ForgeApi::new(catalog());

    let versions = block_on(api.versions("1.20.1")).expect("versions");

    assert_eq!(
        versions
            .iter()
            .map(|v| v.version.as_str())
            .collect::<Vec<_>>(),
        ["47.4.29", "47.4.28", "47.4.0"]
    );
    assert!(versions.iter().all(|entry| entry.stable.is_none()));
}

#[test]
fn the_catalog_marks_only_the_recommended_build() {
    let mut network = catalog();
    network
        .responses
        .insert(PROMOTIONS.into(), Ok(PROMOTIONS_BODY.as_bytes().to_vec()));

    let versions = block_on(ForgeApi::new(network).versions("1.20.1")).unwrap();

    let recommended: Vec<_> = versions
        .iter()
        .filter(|entry| entry.stable == Some(true))
        .map(|entry| entry.version.as_str())
        .collect();
    assert_eq!(recommended, ["47.4.28"]);
}

#[test]
fn a_game_version_without_maven_builds_is_no_versions() {
    let api = ForgeApi::new(catalog());

    let error = block_on(api.versions("1.5.2")).expect_err("none");

    assert!(matches!(error, LoaderError::NoVersions { .. }), "{error}");
}

#[test]
fn a_non_maven_response_is_a_metadata_error() {
    let network = FakeNetwork {
        responses: BTreeMap::from([(
            format!("{MAVEN}/maven-metadata.xml"),
            Ok(b"<html>Service unavailable</html>".to_vec()),
        )]),
        ..FakeNetwork::default()
    };
    let error = block_on(ForgeApi::new(network).versions("1.20.1")).unwrap_err();
    assert!(matches!(error, LoaderError::Malformed { .. }));
}

#[test]
fn the_catalog_deduplicates_repeated_game_suffixes_and_preserves_branch_coordinates() {
    let api = ForgeApi::new(catalog());
    let historical = block_on(api.versions("1.7.10")).unwrap();
    assert_eq!(historical.len(), 1);
    assert_eq!(historical[0].version, "10.13.4.1614");
    assert_eq!(
        block_on(api.versions("1.10")).unwrap()[0].version,
        "12.18.0.2000-1.10.0"
    );
    assert_eq!(block_on(api.versions("26.1")).unwrap()[0].version, "62.0.2");
    assert_eq!(
        block_on(api.versions("26.1-snapshot-1")).unwrap()[0].version,
        "62.0.1"
    );
}

#[test]
fn the_profile_comes_from_the_installer_of_the_recommended_version() {
    let mut network = catalog();
    network
        .responses
        .insert(PROMOTIONS.into(), Ok(PROMOTIONS_BODY.as_bytes().to_vec()));
    network.responses.insert(
        format!("{MAVEN}/1.20.1-47.4.28/forge-1.20.1-47.4.28-installer.jar"),
        Ok(installer("1.20.1", "1.20.1-forge-47.4.28")),
    );
    let api = ForgeApi::new(network);

    let profile = block_on(api.profile(LoaderRequest {
        game_version: "1.20.1".into(),
        loader_version: None,
    }))
    .expect("profile");

    assert_eq!(profile.version_id, "1.20.1-forge-47.4.28");
}

#[test]
fn a_named_loader_version_is_fetched_even_when_no_promotion_names_it() {
    let mut network = catalog();
    network.responses.insert(
        format!("{MAVEN}/1.20.1-47.4.0/forge-1.20.1-47.4.0-installer.jar"),
        Ok(installer("1.20.1", "1.20.1-forge-47.4.0")),
    );
    let api = ForgeApi::new(network);

    let profile = block_on(api.profile(LoaderRequest {
        game_version: "1.20.1".into(),
        loader_version: Some("47.4.0".into()),
    }))
    .expect("profile");

    assert_eq!(profile.version_id, "1.20.1-forge-47.4.0");
}

#[test]
fn a_historical_build_goes_directly_to_its_published_coordinate() {
    let coordinate = "1.7.10-10.13.4.1614-1.7.10";
    let profile_id = "1.7.10-Forge10.13.4.1614-1.7.10";
    let mut network = catalog();
    network.responses.insert(
        format!("{MAVEN}/{coordinate}/forge-{coordinate}-installer.jar"),
        Ok(installer("1.7.10", profile_id)),
    );
    let api = ForgeApi::new(network);
    let listed = block_on(api.versions("1.7.10")).unwrap();
    assert_eq!(listed[0].version, "10.13.4.1614");
    for version in [listed[0].version.as_str(), coordinate] {
        let profile = block_on(api.profile(LoaderRequest {
            game_version: "1.7.10".into(),
            loader_version: Some(version.into()),
        }))
        .unwrap();
        assert_eq!(profile.version_id, profile_id);
    }
}

#[test]
fn a_named_installer_remains_reachable_when_the_index_is_down_or_malformed() {
    let profile_id = "1.20.1-forge-47.4.0";
    let coordinate = "1.20.1-47.4.0";
    for (metadata, request) in [
        (Err(FetchError::Retryable("HTTP 503".into())), "47.4.0"),
        (Ok(b"<html>Service unavailable</html>".to_vec()), coordinate),
    ] {
        let api = ForgeApi::new(FakeNetwork {
            responses: BTreeMap::from([
                (format!("{MAVEN}/maven-metadata.xml"), metadata),
                (
                    format!("{MAVEN}/{coordinate}/forge-{coordinate}-installer.jar"),
                    Ok(installer("1.20.1", profile_id)),
                ),
            ]),
            ..FakeNetwork::default()
        });
        let profile = block_on(api.profile(LoaderRequest {
            game_version: "1.20.1".into(),
            loader_version: Some(request.into()),
        }))
        .unwrap();
        assert_eq!(profile.version_id, profile_id);
    }
}

#[test]
fn a_missing_historical_installer_can_try_its_other_coordinate() {
    let short = "1.7.10-10.13.4.1614";
    let coordinate = "1.7.10-10.13.4.1614-1.7.10";
    let profile_id = "1.7.10-Forge10.13.4.1614-1.7.10";
    for status in [404, 403] {
        let mut network = catalog();
        network.responses.extend([
            (
                format!("{MAVEN}/{short}/forge-{short}-installer.jar"),
                Err(FetchError::Rejected {
                    status,
                    body: Vec::new(),
                }),
            ),
            (
                format!("{MAVEN}/{coordinate}/forge-{coordinate}-installer.jar"),
                Ok(installer("1.7.10", profile_id)),
            ),
        ]);
        let result = block_on(ForgeApi::new(network).profile(LoaderRequest {
            game_version: "1.7.10".into(),
            loader_version: Some(short.into()),
        }));
        if status == 404 {
            assert_eq!(result.unwrap().version_id, profile_id);
        } else {
            assert!(matches!(
                result.unwrap_err(),
                LoaderError::Unavailable {
                    source: FetchError::Rejected { status: 403, .. },
                    ..
                }
            ));
        }
    }
}

#[test]
fn a_stale_promotion_uses_the_same_listing_to_choose_a_build() {
    let mut network = catalog();
    network.responses.extend([
        (
            PROMOTIONS.into(),
            Ok(br#"{"promos":{"1.20.1-recommended":"47.0.0"}}"#.to_vec()),
        ),
        (
            format!("{MAVEN}/1.20.1-47.4.29/forge-1.20.1-47.4.29-installer.jar"),
            Ok(installer("1.20.1", "1.20.1-forge-47.4.29")),
        ),
    ]);
    let api = ForgeApi::new(network);
    let profile = block_on(api.profile(LoaderRequest {
        game_version: "1.20.1".into(),
        loader_version: None,
    }))
    .unwrap();
    assert_eq!(profile.version_id, "1.20.1-forge-47.4.29");
    assert_eq!(api.network.metadata_requests.get(), 1);
}
