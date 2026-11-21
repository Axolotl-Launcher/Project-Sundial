// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! The game files a version document names that the game root does not hold.
//!
//! Planning is separate from downloading: the planners only read the
//! filesystem and return what is absent or the wrong size, so they run without
//! a network and are tested against a fixture directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::stream::{self, StreamExt};
use serde::Deserialize;
use superlogical_launch::rules::{self, Target};
use superlogical_launch::version::Library;
use superlogical_network::verify;
use superlogical_spi::{FetchError, Network};
use superlogical_storage::write_atomic;

/// The directory under the game root that holds library artifacts.
const LIBRARIES_DIR: &str = "libraries";

/// The directory under the game root that holds the asset store.
const ASSETS_DIR: &str = "assets";

/// The directory under `assets` that holds content-addressed objects.
const OBJECTS_DIR: &str = "objects";

/// How many leading hash characters select an asset object's directory.
const OBJECT_PREFIX_LEN: usize = 2;

/// Where Mojang serves asset objects, as the asset index format defines them.
const ASSET_OBJECT_BASE: &str = "https://resources.download.minecraft.net";

/// The number of downloads in flight when a caller passes zero.
pub const DEFAULT_CONCURRENCY: usize = 4;

/// One file a version needs that is not in place yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingFile {
    pub url: String,
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

/// The asset index document, reduced to the object map an install reads.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AssetIndexDocument {
    pub objects: BTreeMap<String, AssetObject>,
}

/// One object in an asset index.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

impl AssetIndexDocument {
    /// The document encoded in `bytes`, as Mojang publishes it.
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

/// The library artifacts `libraries` names for `target` that are absent or the
/// wrong size under `game_root`.
pub fn missing_libraries(
    game_root: &Path,
    libraries: &[Library],
    target: &Target,
) -> Vec<MissingFile> {
    let root = game_root.join(LIBRARIES_DIR);
    libraries
        .iter()
        .filter(|library| rules::library_applies(library, target))
        .filter_map(|library| library.downloads.as_ref()?.artifact.as_ref())
        .filter_map(|artifact| {
            let path = root.join(&artifact.path);
            needs_download(&path, artifact.size).then(|| MissingFile {
                // An artifact without a URL stays in the plan, so the download
                // reports it instead of the plan hiding it.
                url: artifact.url.clone().unwrap_or_default(),
                path,
                sha1: artifact.sha1.clone(),
                size: artifact.size,
            })
        })
        .collect()
}

/// The asset objects `index` names that are absent or the wrong size under
/// `game_root`.
pub fn missing_assets(game_root: &Path, index: &AssetIndexDocument) -> Vec<MissingFile> {
    let root = game_root.join(ASSETS_DIR).join(OBJECTS_DIR);
    index
        .objects
        .values()
        .filter_map(|object| {
            let prefix = object.hash.get(..OBJECT_PREFIX_LEN)?;
            let hash = &object.hash;
            let path = root.join(prefix).join(hash);
            needs_download(&path, Some(object.size)).then(|| MissingFile {
                url: format!("{ASSET_OBJECT_BASE}/{prefix}/{hash}"),
                path,
                sha1: Some(hash.clone()),
                size: Some(object.size),
            })
        })
        .collect()
}

/// Whether `path` is absent or has a size other than `expected_size`.
///
/// A present file with no expected size counts as complete: there is nothing to
/// compare against.
fn needs_download(path: &Path, expected_size: Option<u64>) -> bool {
    let existing = fs::metadata(path).map(|metadata| metadata.len()).ok();
    match (existing, expected_size) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(size), Some(expected)) => size != expected,
    }
}

/// What happened to one file in a download run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome {
    pub path: PathBuf,
    pub result: Result<(), String>,
}

/// Fetches every file in `files`, writing only those whose SHA-1 matches.
///
/// At most `concurrency` requests are in flight at once; zero means
/// [`DEFAULT_CONCURRENCY`]. Outcomes arrive in completion order.
pub async fn download_all(
    network: &impl Network,
    files: &[MissingFile],
    concurrency: usize,
) -> Vec<DownloadOutcome> {
    let limit = if concurrency == 0 {
        DEFAULT_CONCURRENCY
    } else {
        concurrency
    };
    stream::iter(files)
        .map(|file| download(network, file))
        .buffer_unordered(limit)
        .collect()
        .await
}

/// The outcome of fetching and writing one file.
async fn download(network: &impl Network, file: &MissingFile) -> DownloadOutcome {
    DownloadOutcome {
        path: file.path.clone(),
        result: fetch_and_write(network, file).await,
    }
}

async fn fetch_and_write(network: &impl Network, file: &MissingFile) -> Result<(), String> {
    let bytes = network.fetch(&file.url).await.map_err(reason)?;
    if let Some(expected) = &file.sha1
        && !verify(&bytes, expected)
    {
        return Err(format!("SHA-1 mismatch for {}", file.path.display()));
    }
    write_atomic(&file.path, &bytes)
        .map_err(|error| format!("cannot write {}: {error}", file.path.display()))
}

/// The text of a fetch error, with the retry advice dropped.
fn reason(error: FetchError) -> String {
    match error {
        FetchError::Retryable(reason) | FetchError::Fatal(reason) => reason,
    }
}

/// Holds every request open across several polls and records the peak overlap.
#[cfg(test)]
struct SlowNetwork {
    body: Vec<u8>,
    calls: AtomicUsize,
    in_flight: AtomicUsize,
    peak: AtomicUsize,
}

#[cfg(test)]
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

#[cfg(test)]
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
#[cfg(test)]
struct YieldOnce {
    yielded: bool,
}

#[cfg(test)]
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

#[cfg(test)]
fn yield_once() -> YieldOnce {
    YieldOnce { yielded: false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use futures::executor::block_on;
    use superlogical_launch::version::{Artifact, LibraryDownloads, OsRule, Rule};

    /// SHA-1 of `abc`, as `sha1sum` reports it.
    const ABC_SHA1: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";
    /// SHA-1 of `superlogical`, as `sha1sum` reports it.
    const WORD_SHA1: &str = "5adaca6d9d8bf39646d2f71ce5bdf39df55d6b9f";
    const ABC: &[u8] = b"abc";
    const WORD: &[u8] = b"superlogical";
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
}
