// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::cell::{Cell, RefCell};
use std::fs;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::Mutex;
use std::time::Duration;

use axlauncher_spi::{
    ByteRange, DownloadBody, DownloadProtocol, DownloadResponse, FetchError, Network, ResponseRange,
};
use bytes::Bytes;
use futures::executor::block_on;
use futures::{FutureExt, poll};

use crate::resources::Quota;
use crate::{DownloadEngine, DownloadJob, DownloadPolicy, test_tempfile};

const MIB: u64 = 1024 * 1024;
const LENGTH: u64 = 16 * MIB;
const SMALL_FILE: u64 = 64 * 1024 + 1;
const DATA_SOURCE: &str = "https://example.invalid/file";

#[test]
fn default_policy_uses_http1_and_the_expected_connection_budgets() {
    let policy = DownloadPolicy::default();
    assert_eq!(policy.protocol, DownloadProtocol::Http1);
    assert_eq!(policy.connections.get(), 256);
    assert_eq!(policy.connections_per_file.get(), 16);
}

#[derive(Default)]
struct RangeNetwork {
    requests: RefCell<Vec<Option<ByteRange>>>,
    fail_once: Cell<bool>,
    ignore_later_ranges: bool,
    length: Option<u64>,
    protocol: Option<DownloadProtocol>,
}

impl Network for RangeNetwork {
    async fn fetch(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        panic!("file downloads use the streaming port")
    }

    async fn open_download(
        &self,
        _url: &str,
        protocol: DownloadProtocol,
        range: Option<ByteRange>,
    ) -> Result<DownloadResponse<impl DownloadBody>, FetchError> {
        assert_eq!(protocol, self.protocol.unwrap_or_default());
        self.requests.borrow_mut().push(range);
        let total = self.length.unwrap_or(LENGTH);
        let interval = range.unwrap_or(ByteRange {
            start: 0,
            end: total,
        });
        let interrupted = interval.start == 8 * MIB && self.fail_once.replace(false);
        let ignored = self.ignore_later_ranges && interval.start > 0;
        let range = range
            .filter(|_| !ignored)
            .map(|interval| ResponseRange { interval, total });
        let length = if ignored {
            total
        } else {
            interval.end - interval.start
        };
        Ok(DownloadResponse {
            length: Some(length),
            range,
            body: PatternBody {
                offset: if ignored { 0 } else { interval.start },
                end: if ignored { total } else { interval.end },
                first: true,
                interrupted,
                version: u8::from(self.ignore_later_ranges && range.is_none()),
            },
        })
    }
}

struct PatternBody {
    offset: u64,
    end: u64,
    first: bool,
    interrupted: bool,
    version: u8,
}

impl DownloadBody for PatternBody {
    async fn next_chunk(&mut self) -> Result<Option<Bytes>, FetchError> {
        if self.interrupted && !self.first {
            return Err(FetchError::Retryable("connection closed".into()));
        }
        self.first = false;
        let end = self.end.min(self.offset + 64 * 1024);
        let bytes: Vec<_> = (self.offset..end)
            .map(|offset| pattern(offset, self.version))
            .collect();
        self.offset = end;
        Ok((!bytes.is_empty()).then(|| Bytes::from(bytes)))
    }
}

fn pattern(offset: u64, version: u8) -> u8 {
    ((offset % 251) as u8).wrapping_add(version)
}

fn assert_contents(path: &std::path::Path, version: u8) {
    let body = fs::read(path).unwrap();
    assert_eq!(body.len() as u64, LENGTH);
    assert!(
        body.iter()
            .copied()
            .enumerate()
            .all(|(offset, byte)| byte == pattern(offset as u64, version))
    );
}

#[test]
fn the_first_known_range_is_eight_mib_and_an_interrupted_tail_resumes() {
    let root = test_tempfile::tempdir().unwrap();
    let destination = root.path().join("file");
    let network = RangeNetwork {
        fail_once: Cell::new(true),
        ..RangeNetwork::default()
    };
    let engine = DownloadEngine::new(DownloadPolicy {
        connections_per_file: NonZeroUsize::new(2).unwrap(),
        ..DownloadPolicy::default()
    });
    let job = DownloadJob {
        sources: &[DATA_SOURCE],
        destination: &destination,
        sha1: None,
        size: Some(LENGTH),
        batch: true,
    };

    block_on(engine.download(&network, job)).unwrap();

    let requests = network.requests.into_inner();
    assert_eq!(
        requests[0],
        Some(ByteRange {
            start: 0,
            end: 8 * MIB
        })
    );
    assert!(requests.contains(&Some(ByteRange {
        start: 8 * MIB,
        end: LENGTH
    })));
    assert!(
        requests
            .iter()
            .flatten()
            .any(|range| range.start == 8 * MIB + 64 * 1024)
    );
    assert_eq!(engine.stats().bytes_received, LENGTH);
    assert_contents(&destination, 0);
}

#[test]
fn a_small_http1_file_uses_parallel_ranges_without_a_tiny_tail() {
    let root = test_tempfile::tempdir().unwrap();
    let destination = root.path().join("file");
    let network = RangeNetwork {
        length: Some(SMALL_FILE),
        ..RangeNetwork::default()
    };
    let job = DownloadJob {
        sources: &[DATA_SOURCE],
        destination: &destination,
        sha1: None,
        size: Some(SMALL_FILE),
        batch: true,
    };

    block_on(DownloadEngine::default().download(&network, job)).unwrap();

    let requests = network.requests.into_inner();
    assert!(requests.len() > 1);
    assert!(requests.into_iter().all(|range| {
        let range = range.unwrap();
        range.end - range.start >= crate::weighted::MIN_RANGE_BYTES
    }));
    let contents = fs::read(destination).unwrap();
    assert_eq!(contents.len() as u64, SMALL_FILE);
    assert!(
        contents
            .into_iter()
            .enumerate()
            .all(|(offset, byte)| byte == pattern(offset as u64, 0))
    );
}

#[test]
fn explicitly_selected_http2_keeps_small_files_on_a_single_request() {
    let root = test_tempfile::tempdir().unwrap();
    let destination = root.path().join("file");
    let protocol = DownloadProtocol::Http2;
    let network = RangeNetwork {
        length: Some(SMALL_FILE),
        protocol: Some(protocol),
        ..RangeNetwork::default()
    };
    let policy = DownloadPolicy {
        protocol,
        ..DownloadPolicy::default()
    };
    let job = DownloadJob {
        sources: &[DATA_SOURCE],
        destination: &destination,
        sha1: None,
        size: Some(SMALL_FILE),
        batch: true,
    };

    block_on(DownloadEngine::new(policy).download(&network, job)).unwrap();

    assert_eq!(network.requests.into_inner(), [None]);
    assert_eq!(fs::metadata(destination).unwrap().len(), SMALL_FILE);
}

#[test]
fn an_ignored_range_restarts_as_a_whole_file() {
    let root = test_tempfile::tempdir().unwrap();
    let destination = root.path().join("file");
    let network = RangeNetwork {
        ignore_later_ranges: true,
        ..RangeNetwork::default()
    };
    let engine = DownloadEngine::default();
    let job = DownloadJob {
        sources: &[DATA_SOURCE],
        destination: &destination,
        sha1: None,
        size: None,
        batch: false,
    };

    block_on(engine.download(&network, job)).unwrap();

    let requests = network.requests.into_inner();
    assert_eq!(requests[0], Some(ByteRange { start: 0, end: 1 }));
    assert_eq!(requests.last(), Some(&None));
    assert_contents(&destination, 1);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn multipart_downloads_accept_all_protocols_without_a_digest() {
    for protocol in [
        DownloadProtocol::Http1,
        DownloadProtocol::Http2,
        DownloadProtocol::Automatic,
    ] {
        assert_multipart_without_digest(protocol);
    }
}

fn assert_multipart_without_digest(protocol: DownloadProtocol) {
    let root = test_tempfile::tempdir().unwrap();
    let destination = root.path().join("file");
    let network = RangeNetwork {
        protocol: Some(protocol),
        ..RangeNetwork::default()
    };
    let engine = DownloadEngine::new(DownloadPolicy {
        protocol,
        ..DownloadPolicy::default()
    });
    let job = DownloadJob {
        sources: &[DATA_SOURCE],
        destination: &destination,
        sha1: None,
        size: Some(LENGTH),
        batch: true,
    };

    block_on(engine.download(&network, job)).unwrap();

    let requests = network.requests.into_inner();
    assert!(requests.len() > 1);
    assert!(requests.iter().all(Option::is_some));
    assert_contents(&destination, 0);
}

#[test]
fn a_batch_with_unknown_size_does_not_probe() {
    let root = test_tempfile::tempdir().unwrap();
    let destination = root.path().join("file");
    let network = RangeNetwork::default();
    let job = DownloadJob {
        sources: &[DATA_SOURCE],
        destination: &destination,
        sha1: None,
        size: None,
        batch: true,
    };

    block_on(DownloadEngine::default().download(&network, job)).unwrap();

    assert_eq!(network.requests.into_inner(), vec![None]);
    assert_contents(&destination, 0);
}

struct Alternates(Mutex<Vec<String>>);

impl Network for Alternates {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.0.lock().unwrap().push(url.to_owned());
        if url == DATA_SOURCE {
            return Ok(b"abc".to_vec());
        }
        Err(FetchError::Rejected {
            status: 404,
            body: Vec::new(),
        })
    }
}

#[test]
fn source_fallback_and_hash_reuse_preserve_a_verified_existing_file() {
    let root = test_tempfile::tempdir().unwrap();
    let destination = root.path().join("file");
    fs::write(&destination, b"old").unwrap();
    let network = Alternates(Mutex::default());
    let first = "https://alternate.invalid/file";
    let job = DownloadJob {
        sources: &[first, DATA_SOURCE],
        destination: &destination,
        sha1: Some("a9993e364706816aba3e25717850c26c9cd0d89d"),
        size: Some(4),
        batch: true,
    };
    let engine = DownloadEngine::default();

    block_on(engine.download(&network, job)).unwrap();
    block_on(engine.download(&network, job)).unwrap();

    assert_eq!(network.0.into_inner().unwrap(), [first, DATA_SOURCE]);
    assert_eq!(fs::read(&destination).unwrap(), b"abc");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn a_connection_increase_admits_waiters_before_the_first_releases() {
    block_on(async {
        let quota = Quota::new(NonZeroUsize::MIN);
        let first = quota.acquire().await;
        let mut second = quota.acquire().boxed();
        assert!(poll!(&mut second).is_pending());
        quota.set_limit(NonZeroUsize::new(2).unwrap());
        let second = second.await;
        assert_eq!(quota.active(), 2);
        drop((first, second));
        assert_eq!(quota.active(), 0);
    });
}

#[test]
fn disabling_a_speed_limit_wakes_pending_reads() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let engine = DownloadEngine::new(DownloadPolicy {
                bytes_per_second: Some(NonZeroU64::MIN),
                ..DownloadPolicy::default()
            });
            let mut charge = engine.bandwidth.acquire(64 * 1024).boxed();
            assert!(poll!(&mut charge).is_pending());
            engine.set_speed_limit(None);
            tokio::time::timeout(Duration::from_secs(1), charge)
                .await
                .unwrap();
        });
}

#[test]
fn simultaneous_reads_share_one_bandwidth_budget() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            let rate = NonZeroU64::new(32 * 1024).unwrap();
            let started = tokio::time::Instant::now();
            let bandwidth = crate::resources::Bandwidth::new(Some(rate));

            futures::join!(bandwidth.acquire(4096), bandwidth.acquire(4096));

            assert!(started.elapsed() >= Duration::from_millis(250));
        });
}
