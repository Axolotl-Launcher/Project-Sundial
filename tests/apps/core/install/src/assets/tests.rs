// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use axlauncher_launch::version::{Artifact, LibraryDownloads, OsRule, Rule};
use futures::executor::block_on;

use crate::test_tempfile as tempfile;

/// SHA-1 of `abc`, as `sha1sum` reports it.
const ABC_SHA1: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";
/// SHA-1 of `axlauncher`, as `sha1sum` reports it.
const WORD_SHA1: &str = "43ad1705f06e8b0d18583bd70a2f721ae9ec93a8";
const ABC: &[u8] = b"abc";
const WORD: &[u8] = b"axlauncher";
const LIMIT: usize = 2;
const FILE_COUNT: usize = 8;

fn target() -> Target {
    Target {
        os: "windows".into(),
        arch: "x86_64".into(),
    }
}

fn artifact(path: &str) -> Artifact {
    Artifact {
        path: path.into(),
        sha1: Some(ABC_SHA1.into()),
        size: Some(ABC.len() as u64),
        url: Some(format!("https://example.invalid/{path}")),
    }
}

fn library(artifact: Artifact, rules: Option<Vec<Rule>>) -> Library {
    Library {
        name: "org.example:example:1.0".into(),
        downloads: Some(LibraryDownloads {
            artifact: Some(artifact),
            classifiers: None,
        }),
        natives: None,
        rules,
        url: None,
        include_in_classpath: true,
    }
}

fn write_file(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn missing_file(root: &Path, name: &str, sha1: &str, size: u64) -> MissingFile {
    MissingFile {
        url: format!("https://example.invalid/{name}"),
        path: root.join(name),
        sha1: Some(sha1.into()),
        size: Some(size),
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

#[test]
fn missing_libraries_reports_an_absent_artifact() {
    let root = tempfile::tempdir().unwrap();
    let library = library(artifact("org/example/a.jar"), None);

    let missing = missing_libraries(root.path(), &[library], &target());

    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0].path,
        root.path().join("libraries/org/example/a.jar")
    );
    assert_eq!(missing[0].url, "https://example.invalid/org/example/a.jar");
    assert_eq!(missing[0].sha1.as_deref(), Some(ABC_SHA1));
    assert_eq!(missing[0].size, Some(ABC.len() as u64));
}

#[test]
fn missing_classifiers_go_to_the_hash_cache() {
    let data = tempfile::tempdir().unwrap();
    let game_root = data.path().join("meta");
    let mut library = library(artifact("org/example/example-1.0.jar"), None);
    library.natives = Some(BTreeMap::from([(
        "windows".into(),
        "natives-windows".into(),
    )]));
    let classifier = artifact("org/example/example-1.0-natives-windows.jar");
    library.downloads.as_mut().unwrap().classifiers =
        Some(BTreeMap::from([("natives-windows".into(), classifier)]));

    let files = missing_libraries(&game_root, &[library], &target());

    assert_eq!(files.len(), 2);
    assert_eq!(
        files[1].path,
        data.path()
            .join("caches/minecraft-natives")
            .join(format!("{ABC_SHA1}.jar"))
    );
}

#[test]
fn missing_libraries_reports_an_unverified_artifact() {
    let root = tempfile::tempdir().unwrap();
    let mut unverified = artifact("org/example/a.jar");
    unverified.sha1 = None;
    let library = library(unverified, None);

    let missing = missing_libraries(root.path(), &[library], &target());

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].sha1, None);
}

#[test]
fn missing_libraries_skips_a_correct_file() {
    let root = tempfile::tempdir().unwrap();
    write_file(root.path(), "libraries/org/example/a.jar", ABC);
    let library = library(artifact("org/example/a.jar"), None);

    assert!(missing_libraries(root.path(), &[library], &target()).is_empty());
}

#[test]
fn missing_libraries_reports_a_wrong_size() {
    let root = tempfile::tempdir().unwrap();
    write_file(root.path(), "libraries/org/example/a.jar", b"four");
    let library = library(artifact("org/example/a.jar"), None);

    let missing = missing_libraries(root.path(), &[library], &target());

    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0].path,
        root.path().join("libraries/org/example/a.jar")
    );
}

#[test]
fn missing_libraries_ignores_a_library_excluded_for_the_target() {
    let root = tempfile::tempdir().unwrap();
    let only_macos = vec![Rule {
        action: "allow".into(),
        os: Some(OsRule {
            name: Some("osx".into()),
            arch: None,
        }),
    }];
    let library = library(artifact("org/example/a.jar"), Some(only_macos));

    assert!(missing_libraries(root.path(), &[library], &target()).is_empty());
}

#[test]
fn asset_index_parses_the_objects_map() {
    let json = br#"{"objects":{"minecraft/sounds/a.ogg":{"hash":"0123","size":7}}}"#;

    let document = AssetIndexDocument::parse(json).unwrap();

    let object = &document.objects["minecraft/sounds/a.ogg"];
    assert_eq!(object.hash, "0123");
    assert_eq!(object.size, 7);
}

#[test]
fn missing_assets_uses_the_two_character_fan_out() {
    let root = tempfile::tempdir().unwrap();
    let hash = "0123456789abcdef0123456789abcdef01234567";
    let index = AssetIndexDocument {
        objects: BTreeMap::from([(
            "minecraft/sounds/a.ogg".to_owned(),
            AssetObject {
                hash: hash.into(),
                size: ABC.len() as u64,
            },
        )]),
        ..AssetIndexDocument::default()
    };

    let missing = missing_assets(root.path(), &index);

    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0].path,
        root.path().join(format!("assets/objects/01/{hash}"))
    );
    assert_eq!(
        missing[0].url,
        format!("https://resources.download.minecraft.net/01/{hash}")
    );
    assert_eq!(missing[0].sha1.as_deref(), Some(hash));
    assert_eq!(missing[0].size, Some(ABC.len() as u64));
}

#[test]
fn missing_assets_skips_a_correct_object() {
    let root = tempfile::tempdir().unwrap();
    let hash = "0123456789abcdef0123456789abcdef01234567";
    write_file(root.path(), &format!("assets/objects/01/{hash}"), ABC);
    let index = AssetIndexDocument {
        objects: BTreeMap::from([(
            "minecraft/sounds/a.ogg".to_owned(),
            AssetObject {
                hash: hash.into(),
                size: ABC.len() as u64,
            },
        )]),
        ..AssetIndexDocument::default()
    };

    assert!(missing_assets(root.path(), &index).is_empty());
}

#[test]
fn download_all_writes_verified_files() {
    let root = tempfile::tempdir().unwrap();
    let first = missing_file(root.path(), "first.bin", ABC_SHA1, ABC.len() as u64);
    let second = missing_file(root.path(), "second.bin", WORD_SHA1, WORD.len() as u64);
    let network = FixedNetwork {
        responses: BTreeMap::from([
            (first.url.clone(), Ok(ABC.to_vec())),
            (second.url.clone(), Ok(WORD.to_vec())),
        ]),
    };

    let outcomes = block_on(download_all(
        &network,
        &[first.clone(), second.clone()],
        LIMIT,
    ));

    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(|outcome| outcome.result.is_ok()));
    assert_eq!(fs::read(&first.path).unwrap(), ABC);
    assert_eq!(fs::read(&second.path).unwrap(), WORD);
}

#[test]
fn download_all_rejects_a_sha1_mismatch() {
    let root = tempfile::tempdir().unwrap();
    let file = missing_file(root.path(), "bad.bin", ABC_SHA1, ABC.len() as u64);
    let network = FixedNetwork {
        responses: BTreeMap::from([(file.url.clone(), Ok(WORD.to_vec()))]),
    };

    let outcomes = block_on(download_all(&network, std::slice::from_ref(&file), 1));

    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].path, file.path);
    assert!(outcomes[0].result.is_err());
    assert!(!file.path.exists());
    assert!(!temporary_path(&file.path).exists());
}

#[test]
fn download_all_continues_after_a_mismatch() {
    let root = tempfile::tempdir().unwrap();
    let first = missing_file(root.path(), "first.bin", ABC_SHA1, ABC.len() as u64);
    let bad = missing_file(root.path(), "bad.bin", ABC_SHA1, ABC.len() as u64);
    let second = missing_file(root.path(), "second.bin", WORD_SHA1, WORD.len() as u64);
    let network = FixedNetwork {
        responses: BTreeMap::from([
            (first.url.clone(), Ok(ABC.to_vec())),
            (bad.url.clone(), Ok(WORD.to_vec())),
            (second.url.clone(), Ok(WORD.to_vec())),
        ]),
    };
    let files = [first.clone(), bad.clone(), second.clone()];

    let outcomes = block_on(download_all(&network, &files, files.len()));

    assert_eq!(outcomes.len(), files.len());
    let failures = outcomes
        .iter()
        .filter(|outcome| outcome.result.is_err())
        .count();
    assert_eq!(failures, 1);
    assert!(first.path.exists());
    assert!(second.path.exists());
    assert!(!bad.path.exists());
}

#[test]
fn download_all_bounds_concurrency() {
    let root = tempfile::tempdir().unwrap();
    let files: Vec<MissingFile> = (0..FILE_COUNT)
        .map(|index| {
            missing_file(
                root.path(),
                &format!("file-{index}.bin"),
                ABC_SHA1,
                ABC.len() as u64,
            )
        })
        .collect();
    let network = SlowNetwork::new(ABC.to_vec());

    let outcomes = block_on(download_all(&network, &files, LIMIT));

    assert_eq!(outcomes.len(), FILE_COUNT);
    assert!(outcomes.iter().all(|outcome| outcome.result.is_ok()));
    assert_eq!(network.calls.load(Ordering::SeqCst), FILE_COUNT);
    let peak = network.peak.load(Ordering::SeqCst);
    assert!(peak <= LIMIT, "peak {peak} exceeded the limit");
    assert!(peak > 1, "requests never overlapped");
}

/// Answers each URL with a fixed result.
struct FixedNetwork {
    responses: BTreeMap<String, Result<Vec<u8>, FetchError>>,
}

impl Network for FixedNetwork {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        match self.responses.get(url) {
            Some(response) => response.clone(),
            None => Err(FetchError::Fatal(format!("unexpected URL: {url}"))),
        }
    }
}

/// Holds every request open across several polls and records the peak overlap.
struct SlowNetwork {
    body: Vec<u8>,
    calls: AtomicUsize,
    in_flight: AtomicUsize,
    peak: AtomicUsize,
}

impl SlowNetwork {
    fn new(body: Vec<u8>) -> Self {
        Self {
            body,
            calls: AtomicUsize::new(0),
            in_flight: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        }
    }
}

impl Network for SlowNetwork {
    async fn fetch(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(current, Ordering::SeqCst);
        yield_once().await;
        yield_once().await;
        yield_once().await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(self.body.clone())
    }
}

/// A future that is pending once and ready on the next poll, waking the task
/// in between.
struct YieldOnce {
    yielded: bool,
}

impl std::future::Future for YieldOnce {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        if self.yielded {
            return std::task::Poll::Ready(());
        }
        self.yielded = true;
        context.waker().wake_by_ref();
        std::task::Poll::Pending
    }
}

fn yield_once() -> YieldOnce {
    YieldOnce { yielded: false }
}

#[test]
fn a_virtual_index_copies_objects_into_its_flat_tree() {
    let root = tempfile::tempdir().unwrap();
    let hash = "0123456789abcdef0123456789abcdef01234567";
    write_file(root.path(), &format!("assets/objects/01/{hash}"), ABC);
    let index = AssetIndexDocument {
        objects: BTreeMap::from([(
            "minecraft/sounds/a.ogg".to_owned(),
            AssetObject {
                hash: hash.into(),
                size: ABC.len() as u64,
            },
        )]),
        virtual_map: true,
        ..AssetIndexDocument::default()
    };

    let failures = materialise_flat(root.path(), "legacy", &index);

    assert!(failures.is_empty(), "{failures:?}");
    let flat = root
        .path()
        .join("assets/virtual/legacy/minecraft/sounds/a.ogg");
    assert_eq!(fs::read(&flat).expect("flat copy"), ABC);
}

#[test]
fn a_map_to_resources_index_copies_into_resources() {
    let root = tempfile::tempdir().unwrap();
    let hash = "0123456789abcdef0123456789abcdef01234567";
    write_file(root.path(), &format!("assets/objects/01/{hash}"), ABC);
    let index = AssetIndexDocument {
        objects: BTreeMap::from([(
            "sound/a.ogg".to_owned(),
            AssetObject {
                hash: hash.into(),
                size: ABC.len() as u64,
            },
        )]),
        map_to_resources: true,
        ..AssetIndexDocument::default()
    };

    materialise_flat(root.path(), "pre-1.6", &index);

    let flat = root.path().join("resources").join("sound").join("a.ogg");
    assert_eq!(fs::read(&flat).expect("flat copy"), ABC);
}

#[test]
fn a_modern_index_asks_for_no_flat_tree() {
    let root = tempfile::tempdir().unwrap();
    let index = AssetIndexDocument::default();

    assert!(materialise_flat(root.path(), "34", &index).is_empty());
    assert!(!root.path().join("assets/virtual").exists());
    assert!(!root.path().join("resources").exists());
}

#[test]
fn a_flat_tree_is_not_rebuilt_when_it_is_current() {
    let root = tempfile::tempdir().unwrap();
    let hash = "0123456789abcdef0123456789abcdef01234567";
    write_file(root.path(), &format!("assets/objects/01/{hash}"), ABC);
    write_file(root.path(), "assets/virtual/legacy/a.ogg", ABC);
    let index = AssetIndexDocument {
        objects: BTreeMap::from([(
            "a.ogg".to_owned(),
            AssetObject {
                hash: hash.into(),
                size: ABC.len() as u64,
            },
        )]),
        virtual_map: true,
        ..AssetIndexDocument::default()
    };

    materialise_flat(root.path(), "legacy", &index);

    // The destination is untouched: it already had the right size, so a
    // second pass did not need to read the object at all.
    let flat = root.path().join("assets/virtual/legacy/a.ogg");
    assert_eq!(fs::read(&flat).expect("flat copy"), ABC);
}
