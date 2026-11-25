// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::vanilla::catalog::CATALOG;
use axlauncher_spi::FetchError;

use futures::executor::block_on;

const VERSION_URL: &str = "https://example.invalid/version.json";
const CLIENT_URL: &str = "https://example.invalid/client.jar";
const MANIFEST: &[u8] = br#"{"id":"26.3","downloads":{"client":{"sha1":"a9993e364706816aba3e25717850c26c9cd0d89d","size":3,"url":"https://example.invalid/client.jar"}}}"#;
const MANIFEST_SHA1: &str = "6743ed617811a86c4ff82406158679f9a752d624";

struct NetworkFixture(&'static str);

impl Network for NetworkFixture {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        match url {
            CATALOG => Ok(serde_json::to_vec(&serde_json::json!({"versions": [
                {"id":"26.4-snapshot-10", "type":"snapshot", "releaseTime":"2026-11-17T16:02:02+00:00", "url":VERSION_URL, "sha1": self.0},
                {"id":"26.3", "type":"release", "releaseTime":"2026-10-14T11:00:00+00:00", "url":VERSION_URL, "sha1": self.0},
                {"id":"b1.7.3", "type":"old_beta", "releaseTime":"2011-07-07T22:00:00+00:00", "url":VERSION_URL, "sha1": self.0},
                {"id":"rd-132211", "type":"old_alpha", "releaseTime":"2009-05-13T20:11:00+00:00", "url":VERSION_URL, "sha1": self.0}
            ], "latest": {"release":"26.3", "snapshot":"26.4-snapshot-10"}}))
            .unwrap()),
            VERSION_URL => Ok(MANIFEST.to_vec()),
            CLIENT_URL => Ok(b"abc".to_vec()),
            other => Err(FetchError::Fatal(format!("unexpected URL {other}"))),
        }
    }
}

struct MirrorFixture;

impl Network for MirrorFixture {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Fatal(format!("unclassified request for {url}")))
    }

    async fn fetch_with<T, E>(
        &self,
        url: &str,
        kind: FetchKind,
        accept: impl Fn(Result<Vec<u8>, FetchError>) -> Result<T, E>,
    ) -> Result<T, E> {
        let (required_policy, mirrored) = match url {
            CATALOG => (
                FetchKind::UnverifiedMetadata,
                br#"{"latest":{"release":"26.2"},"versions":[]}"#.as_slice(),
            ),
            VERSION_URL => (FetchKind::Metadata, b"corrupt manifest".as_slice()),
            _ => return accept(self.fetch(url).await),
        };
        assert_eq!(kind, required_policy);
        match accept(Ok(mirrored.to_vec())) {
            Ok(value) => Ok(value),
            Err(_) => accept(NetworkFixture(MANIFEST_SHA1).fetch(url).await),
        }
    }
}

#[test]
fn installation_retries_a_stale_catalog_and_corrupt_manifest_at_the_official_source() {
    let root = crate::test_tempfile::tempdir().unwrap();
    block_on(ensure_version(&MirrorFixture, root.path(), "26.3")).unwrap();
    assert_eq!(
        fs::read(root.path().join("versions/26.3/26.3.json")).unwrap(),
        MANIFEST
    );
}

#[test]
fn the_catalog_keeps_official_order_and_historical_categories() {
    let catalog = block_on(versions(&NetworkFixture(MANIFEST_SHA1))).unwrap();
    assert_eq!(catalog.latest_release(), "26.3");
    let listed: Vec<_> = catalog
        .versions()
        .iter()
        .map(|version| (version.id(), version.kind()))
        .collect();
    assert_eq!(
        listed,
        [
            ("26.4-snapshot-10", MinecraftVersionKind::Snapshot),
            ("26.3", MinecraftVersionKind::Release),
            ("b1.7.3", MinecraftVersionKind::OldBeta),
            ("rd-132211", MinecraftVersionKind::OldAlpha),
        ]
    );
}

#[test]
fn a_missing_version_downloads_its_manifest_and_client_and_can_be_retried_offline() {
    use crate::test_tempfile as tempfile;

    let root = tempfile::tempdir().unwrap();
    let mut phases = Vec::new();
    assert!(check_files(root.path(), "26.3", &Target::host()).is_err());
    let installed = block_on(install(
        &NetworkFixture(MANIFEST_SHA1),
        root.path(),
        "26.3",
        &Target::host(),
        1,
        |phase| phases.push(phase),
    ))
    .unwrap();

    assert!(installed.is_complete());
    assert!(check_files(root.path(), "26.3", &Target::host()).is_ok());
    assert_eq!(installed.written, 1);
    assert_eq!(
        fs::read(root.path().join("versions/26.3/26.3.json")).unwrap(),
        MANIFEST
    );
    assert_eq!(
        fs::read(root.path().join("versions/26.3/26.3.jar")).unwrap(),
        b"abc"
    );
    assert!(phases.contains(&Progress::Downloading {
        processed: 1,
        total: 1
    }));

    let retry = block_on(install(
        &axlauncher_network::UnimplementedNetwork,
        root.path(),
        "26.3",
        &Target::host(),
        1,
        |_| {},
    ))
    .unwrap();
    assert!(retry.is_complete());
    assert_eq!(retry.requested, 0);
}

struct PreReleaseCatalog;

const PRE_RELEASE: &str = "1.14 Pre-Release 1";
const PRE_RELEASE_MANIFEST: &[u8] = br#"{"id":"1.14 Pre-Release 1","downloads":{"client":{"size":3,"url":"https://example.invalid/client.jar"}}}"#;

impl Network for PreReleaseCatalog {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        match url {
            CATALOG => Ok(serde_json::to_vec(&serde_json::json!({
                "latest": {"release": "26.3"},
                "versions": [{"id": PRE_RELEASE, "type": "snapshot", "url": VERSION_URL,
                    "sha1": sha1_smol::Sha1::from(PRE_RELEASE_MANIFEST).digest().to_string()}],
            }))
            .unwrap()),
            VERSION_URL => Ok(PRE_RELEASE_MANIFEST.to_vec()),
            CLIENT_URL => Ok(b"abc".to_vec()),
            other => Err(FetchError::Fatal(format!("unexpected URL {other}"))),
        }
    }
}

#[test]
fn a_catalog_pre_release_with_spaces_installs_and_passes_the_offline_check() {
    let root = crate::test_tempfile::tempdir().unwrap();
    let target = Target::host();
    let prepared = block_on(install(
        &PreReleaseCatalog,
        root.path(),
        PRE_RELEASE,
        &target,
        1,
        |_| {},
    ))
    .expect("official-catalog fixture installs");
    assert!(prepared.is_complete(), "{prepared:?}");
    assert_eq!(
        fs::read(
            root.path()
                .join("versions")
                .join(PRE_RELEASE)
                .join(format!("{PRE_RELEASE}.jar"))
        )
        .unwrap(),
        b"abc"
    );
    check_files(root.path(), PRE_RELEASE, &target)
        .expect("launch preflight sees the installed version");
}

#[test]
fn a_manifest_with_the_wrong_hash_is_not_saved() {
    use crate::test_tempfile as tempfile;

    let root = tempfile::tempdir().unwrap();
    let result = block_on(install(
        &NetworkFixture("bad"),
        root.path(),
        "26.3",
        &Target::host(),
        1,
        |_| {},
    ));
    assert!(result.unwrap_err().contains("SHA-1"));
    assert!(!root.path().join("versions").exists());
}

#[test]
fn simultaneous_launches_share_the_game_lock_and_exclude_installation() {
    let root = crate::test_tempfile::tempdir().unwrap();
    let first = launch_lock(root.path()).unwrap();
    let second = launch_lock(root.path()).unwrap();
    assert!(installation_lock(root.path()).is_err());
    drop(first);
    assert!(installation_lock(root.path()).is_err());
    drop(second);
    let install = installation_lock(root.path()).unwrap();
    assert!(launch_lock(root.path()).is_err());
    drop(install);
    assert!(launch_lock(root.path()).is_ok());
}

#[test]
fn offline_check_detects_a_missing_pre_1_6_resource_copy() {
    use crate::test_tempfile as tempfile;

    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    for (relative, content) in [
        (
            "versions/b1.7.3/b1.7.3.json",
            r#"{"id":"b1.7.3","assetIndex":{"id":"pre-1.6","url":""},"downloads":{"client":{"size":3,"url":""}}}"#,
        ),
        ("versions/b1.7.3/b1.7.3.jar", "jar"),
        (
            "assets/indexes/pre-1.6.json",
            r#"{"map_to_resources":true,"objects":{"sound/a.ogg":{"hash":"a9993e364706816aba3e25717850c26c9cd0d89d","size":3}}}"#,
        ),
        (
            "assets/objects/a9/a9993e364706816aba3e25717850c26c9cd0d89d",
            "abc",
        ),
    ] {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    let target = Target::host();
    assert!(
        check_files(root, "b1.7.3", &target)
            .unwrap_err()
            .contains("a.ogg")
    );
    let repair = block_on(install(
        &axlauncher_network::UnimplementedNetwork,
        root,
        "b1.7.3",
        &target,
        1,
        |_| {},
    ))
    .unwrap();
    assert!(repair.is_complete());
    assert_eq!(repair.requested, 0);
    assert!(check_files(root, "b1.7.3", &target).is_ok());
}
