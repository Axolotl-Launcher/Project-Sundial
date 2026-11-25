// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use axlauncher_spi::{DownloadMirror, FetchError};
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

fn missing_file(root: &Path, name: &str, sha1: &str, size: u64) -> MissingFile {
    MissingFile {
        url: format!("https://example.invalid/{name}"),
        path: root.join(name),
        sha1: Some(sha1.into()),
        sha1_alternatives: Vec::new(),
        size: Some(size),
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
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
fn a_corrupt_preferred_mirror_falls_back_before_promoting_the_file() {
    let root = tempfile::tempdir().unwrap();
    let file = missing_file(root.path(), "mirrored.bin", ABC_SHA1, ABC.len() as u64);
    let network = MirroredNetwork {
        preferred: true,
        calls: std::cell::RefCell::default(),
    };
    let outcomes = block_on(download_all(&network, std::slice::from_ref(&file), 1));
    assert!(outcomes[0].result.is_ok());
    assert_eq!(fs::read(&file.path).unwrap(), ABC);
    assert_eq!(
        network.calls.into_inner(),
        ["https://mirror.invalid/file", file.url.as_str()]
    );
}

#[test]
fn the_engine_tries_explicit_sources_before_a_fallback_mirror_and_deduplicates_it() {
    let root = tempfile::tempdir().unwrap();
    let file = missing_file(root.path(), "mirrored.bin", WORD_SHA1, WORD.len() as u64);
    let backup = "https://backup.invalid/file";
    let network = MirroredNetwork {
        preferred: false,
        calls: std::cell::RefCell::default(),
    };
    let result = block_on(DownloadEngine::default().download(
        &network,
        DownloadJob {
            sources: &[&file.url, backup, &file.url],
            destination: &file.path,
            size: file.size,
            sha1: file.sha1.as_deref(),
            sha1_alternatives: &[],
            batch: true,
        },
    ));
    assert!(result.is_ok());
    assert_eq!(fs::read(&file.path).unwrap(), WORD);
    assert_eq!(
        network.calls.into_inner(),
        [file.url.as_str(), backup, "https://mirror.invalid/file"]
    );
}

#[test]
fn additional_verification_rejects_cached_and_downloaded_bytes_before_promotion() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("file.jar");
    fs::write(&path, ABC).unwrap();
    let original = "https://example.invalid/file";
    let backup = "https://mirror.invalid/file";
    let network = MirroredNetwork {
        preferred: false,
        calls: std::cell::RefCell::default(),
    };
    let job = DownloadJob {
        sources: &[original, backup],
        destination: &path,
        size: Some(3),
        sha1: Some(ABC_SHA1),
        sha1_alternatives: &[WORD_SHA1.into()],
        batch: true,
    };
    let engine = DownloadEngine::default();
    let check = |file: &mut fs::File| {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        if bytes == WORD {
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "additional digest mismatch",
        ))
    };
    block_on(engine.download_verified(&network, job, check)).unwrap();
    assert_eq!(fs::read(&path).unwrap(), WORD);
    assert_eq!(*network.calls.borrow(), [original, backup]);
    block_on(engine.download_verified(&network, job, check)).unwrap();
    assert_eq!(*network.calls.borrow(), [original, backup]);
}

struct MirroredNetwork {
    preferred: bool,
    calls: std::cell::RefCell<Vec<String>>,
}

impl Network for MirroredNetwork {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.calls.borrow_mut().push(url.into());
        Ok(if url == "https://mirror.invalid/file" {
            WORD
        } else {
            ABC
        }
        .to_vec())
    }

    fn download_mirror(&self, _url: &str, has_digest: bool) -> Option<DownloadMirror> {
        assert!(has_digest);
        Some(DownloadMirror {
            url: "https://mirror.invalid/file".into(),
            preferred: self.preferred,
        })
    }
}

#[test]
fn download_all_accepts_an_alternative_manifest_digest() {
    let root = tempfile::tempdir().unwrap();
    let mut file = missing_file(root.path(), "alternate.bin", ABC_SHA1, WORD.len() as u64);
    file.sha1_alternatives = vec![WORD_SHA1.to_uppercase()];
    let network = FixedNetwork {
        responses: BTreeMap::from([(file.url.clone(), Ok(WORD.to_vec()))]),
    };
    let outcomes = block_on(download_all(&network, std::slice::from_ref(&file), 1));
    assert!(outcomes[0].result.is_ok(), "{:?}", outcomes[0]);
    assert_eq!(fs::read(&file.path).unwrap(), WORD);
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
