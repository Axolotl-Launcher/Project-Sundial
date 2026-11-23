// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
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
                {"id":"26.3", "url":VERSION_URL, "sha1": self.0}
            ]}))
            .unwrap()),
            VERSION_URL => Ok(MANIFEST.to_vec()),
            CLIENT_URL => Ok(b"abc".to_vec()),
            other => Err(FetchError::Fatal(format!("unexpected URL {other}"))),
        }
    }
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
