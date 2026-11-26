// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use axlauncher_spi::Network;
use std::collections::BTreeMap;
use std::fs;

use crate::test_tempfile as tempfile;

struct FakeNetwork {
    bodies: BTreeMap<String, Vec<u8>>,
}

impl Network for FakeNetwork {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.bodies
            .get(url)
            .cloned()
            .ok_or_else(|| FetchError::Fatal(format!("no body for {url}")))
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = tempfile::tempdir().expect("temp directory").keep();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
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

fn version_with_index(id: &str, url: &str) -> Version {
    Version {
        id: "1.21".into(),
        kind: "release".into(),
        main_class: "net.minecraft.client.main.Main".into(),
        libraries: Vec::new(),
        arguments: None,
        asset_index: Some(AssetIndex {
            id: id.into(),
            url: url.into(),
            sha1: None,
        }),
        assets: None,
        java_version: None,
        minecraft_arguments: None,
        inherits_from: None,
        jar: None,
        downloads: None,
        logging: None,
        processors: Vec::new(),
        data: BTreeMap::default(),
    }
}

#[test]
fn an_absent_index_is_fetched_and_written() {
    let temp = TempDir::new();
    let body = br#"{"objects":{}}"#.to_vec();
    let network = FakeNetwork {
        bodies: BTreeMap::from([("https://example.test/34.json".to_string(), body.clone())]),
    };
    let version = version_with_index("34", "https://example.test/34.json");

    let index = block_on(ensure_asset_index(
        &network,
        &GameDir::standalone(&temp.0),
        &version,
    ))
    .expect("index");

    assert!(index.objects.is_empty());
    let path = temp.0.join("assets/indexes/34.json");
    assert_eq!(fs::read(path).expect("written index"), body);
}

#[test]
fn a_virtual_index_does_not_overwrite_a_pre_1_6_resource() {
    let temp = TempDir::new();
    let game_root = temp.0.join("meta");
    let url = "https://example.test/legacy.json";
    let hash = "a9993e364706816aba3e25717850c26c9cd0d89d";
    let body = format!(
        r#"{{"virtual":true,"objects":{{"sound/a.ogg":{{"hash":"{hash}","size":3}},"sound/b.ogg":{{"hash":"{hash}","size":3}}}}}}"#
    );
    let network = FakeNetwork {
        bodies: BTreeMap::from([
            (url.to_string(), body.into_bytes()),
            (
                format!("https://resources.download.minecraft.net/a9/{hash}"),
                b"abc".to_vec(),
            ),
        ]),
    };
    let mut version = version_with_index("legacy", url);
    version.assets = Some("legacy".into());
    let existing = game_root.join("resources/sound/a.ogg");
    fs::create_dir_all(existing.parent().unwrap()).unwrap();
    fs::write(&existing, b"older sound").unwrap();

    let result = block_on(prepare(
        &network,
        &GameDir::standalone(&game_root),
        &version,
        &Target::host(),
        1,
    ))
    .expect("prepare");

    assert!(result.is_complete(), "{result:?}");
    assert_eq!(result.requested, 1);
    for name in ["a.ogg", "b.ogg"] {
        assert_eq!(
            fs::read(game_root.join("assets/virtual/legacy/sound").join(name)).unwrap(),
            b"abc"
        );
    }
    assert_eq!(fs::read(existing).unwrap(), b"older sound");
}

#[test]
fn an_index_already_on_disk_is_not_fetched_again() {
    let temp = TempDir::new();
    let version = version_with_index("34", "https://example.test/unreachable.json");
    let path = temp.0.join("assets/indexes/34.json");
    fs::create_dir_all(path.parent().expect("parent")).expect("index directory");
    fs::write(&path, br#"{"objects":{}}"#).expect("seed");

    // An empty network: any fetch would fail.
    let network = FakeNetwork {
        bodies: BTreeMap::new(),
    };
    let index = block_on(ensure_asset_index(
        &network,
        &GameDir::standalone(&temp.0),
        &version,
    ))
    .expect("index");

    assert!(index.objects.is_empty());
}

#[test]
fn a_failed_fetch_names_the_index() {
    let temp = TempDir::new();
    let network = FakeNetwork {
        bodies: BTreeMap::new(),
    };
    let version = version_with_index("34", "https://example.test/34.json");

    let error = block_on(ensure_asset_index(
        &network,
        &GameDir::standalone(&temp.0),
        &version,
    ))
    .expect_err("must fail");

    assert!(matches!(error, PrepareError::AssetIndex { .. }));
    assert!(error.to_string().contains("34"));
}

#[test]
fn an_unparsable_index_reports_the_index_id() {
    let temp = TempDir::new();
    let network = FakeNetwork {
        bodies: BTreeMap::from([(
            "https://example.test/34.json".to_string(),
            b"not json".to_vec(),
        )]),
    };
    let version = version_with_index("34", "https://example.test/34.json");

    let error = block_on(ensure_asset_index(
        &network,
        &GameDir::standalone(&temp.0),
        &version,
    ))
    .expect_err("must fail");

    assert!(matches!(error, PrepareError::AssetIndexUnreadable { .. }));
}

#[test]
fn a_version_without_an_index_prepares_nothing_from_assets() {
    let temp = TempDir::new();
    let mut version = version_with_index("34", "https://example.test/34.json");
    version.asset_index = None;
    let network = FakeNetwork {
        bodies: BTreeMap::new(),
    };

    let index = block_on(ensure_asset_index(
        &network,
        &GameDir::standalone(&temp.0),
        &version,
    ))
    .expect("index");

    assert!(index.objects.is_empty());
}

#[test]
fn summarise_counts_written_files_and_names_failures() {
    use crate::assets::DownloadOutcome;

    let outcomes = vec![
        DownloadOutcome {
            path: PathBuf::from("a.jar"),
            result: Ok(()),
        },
        DownloadOutcome {
            path: PathBuf::from("b.jar"),
            result: Err("sha1 mismatch".into()),
        },
    ];

    let prepared = summarise(&outcomes);

    assert_eq!(prepared.requested, 2);
    assert_eq!(prepared.written, 1);
    assert!(!prepared.is_complete());
    assert!(prepared.failures[0].contains("b.jar"));
}

#[test]
fn a_complete_pass_reports_complete() {
    use crate::assets::DownloadOutcome;

    let prepared = summarise(&[DownloadOutcome {
        path: PathBuf::from("a.jar"),
        result: Ok(()),
    }]);

    assert!(prepared.is_complete());
}
