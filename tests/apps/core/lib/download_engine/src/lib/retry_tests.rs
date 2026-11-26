// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::cell::{Cell, RefCell};
use std::fs;
use std::future::Future;
use std::num::NonZeroUsize;
use std::path::Path;
use std::time::Duration;

use axlauncher_spi::downloads::MissingFile;
use axlauncher_spi::{DownloadBody, DownloadResponse, FetchError, RangeRequest, ResponseRange};
use bytes::Bytes;
use futures::{FutureExt, StreamExt, poll};
use tokio::time::Instant;

use super::range_identity::representation;
use super::*;

pub(super) const PRIMARY: &str = "https://primary.invalid/file";
const SAME_ORIGIN: &str = "https://primary.invalid/other";
pub(super) const ALTERNATE: &str = "https://alternate.invalid/file";
pub(super) const SIZE: u64 = 32 * 1024;
pub(super) const OLD_TAG: &[u8] = b"\"old\"";

pub(super) fn paused<T>(test: impl Future<Output = T>) -> T {
    fastrand::seed(123);
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(test)
}

pub(super) fn job<'a>(destination: &'a Path, sources: &'a [&'a str], size: u64) -> DownloadJob<'a> {
    DownloadJob {
        sources,
        destination,
        sha1: None,
        sha1_alternatives: &[],
        size: Some(size),
        batch: true,
    }
}

fn retryable() -> FetchError {
    FetchError::Retryable("connection interrupted".into())
}

fn throttle(status: u16, seconds: u64) -> FetchError {
    FetchError::HttpRetryable {
        status,
        retry_after: Some(Duration::from_secs(seconds)),
    }
}

pub(super) struct Body {
    bytes: Option<Bytes>,
    interrupt: bool,
}

impl DownloadBody for Body {
    async fn next_chunk(&mut self) -> Result<Option<Bytes>, FetchError> {
        let chunk = self.bytes.take();
        if chunk.is_none() && self.interrupt {
            return Err(retryable());
        }
        Ok(chunk)
    }
}

pub(super) fn whole(bytes: impl Into<Bytes>, interrupt: bool) -> DownloadResponse<Body> {
    let bytes = bytes.into();
    DownloadResponse {
        length: Some(bytes.len() as u64),
        range: None,
        representation: None,
        body: Body {
            bytes: Some(bytes),
            interrupt,
        },
    }
}

pub(super) struct Script<F>(pub(super) F);

impl<F> Network for Script<F>
where
    F: Fn(&str, Option<RangeRequest<'_>>) -> Result<DownloadResponse<Body>, FetchError>,
{
    async fn fetch(&self, _: &str) -> Result<Vec<u8>, FetchError> {
        panic!("expected a streaming request")
    }

    async fn open_download(
        &self,
        url: &str,
        _: DownloadProtocol,
        range: Option<RangeRequest<'_>>,
    ) -> Result<DownloadResponse<impl DownloadBody>, FetchError> {
        (self.0)(url, range)
    }
}

#[test]
pub(super) fn whole_recovery_restarts_after_open_and_body_failures() {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let destination = root.path().join("file");
        let times = RefCell::new(Vec::new());
        let network = Script(|_: &str, _: Option<RangeRequest<'_>>| {
            let mut times = times.borrow_mut();
            times.push(Instant::now());
            match times.len() {
                1 => Err(retryable()),
                2 => Ok(whole(b"x".as_slice(), true)),
                3 => Ok(whole(b"new".as_slice(), false)),
                _ => panic!("extra request"),
            }
        });
        let engine = DownloadEngine::default();
        engine
            .download(&network, job(&destination, &[PRIMARY], 3))
            .await
            .unwrap();
        let times = times.into_inner();
        assert!(times.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(engine.stats().bytes_received, 4);
        assert_eq!(fs::read(destination).unwrap(), b"new");
    });
}

#[test]
fn initial_range_and_failed_tails_share_three_recovery_requests() {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let destination = root.path().join("file");
        fs::write(&destination, b"old").unwrap();
        let calls = RefCell::new(Vec::new());
        let network = Script(|_: &str, request: Option<RangeRequest<'_>>| {
            let request = request.expect("expected a range");
            let mut calls = calls.borrow_mut();
            calls.push(request.interval.start);
            let mut response = whole(b"x".as_slice(), true);
            response.range = Some(ResponseRange {
                interval: request.interval,
                total: SIZE,
            });
            response.representation = Some(representation(PRIMARY, Some(OLD_TAG)));
            (calls.len() != 1).then_some(response).ok_or_else(retryable)
        });
        let engine = DownloadEngine::new(DownloadPolicy {
            connections_per_file: NonZeroUsize::MIN,
            ..DownloadPolicy::default()
        });
        let error = engine
            .download(&network, job(&destination, &[PRIMARY], SIZE))
            .await
            .unwrap_err();
        assert_eq!(calls.into_inner(), [0, 0, 1, 2]);
        assert!(error.contains("retry budget exhausted"), "{error}");
        assert!(error.contains("reading download body"), "{error}");
        assert_eq!(engine.stats().active_connections, 0);
        assert_eq!(fs::read(destination).unwrap(), b"old");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    });
}

#[test]
fn an_over_budget_hint_tries_the_next_origin_immediately() {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let destination = root.path().join("file");
        let calls = RefCell::new(Vec::new());
        let network = Script(|url: &str, _: Option<RangeRequest<'_>>| {
            calls.borrow_mut().push((url.to_owned(), Instant::now()));
            (url != PRIMARY)
                .then(|| whole(b"ok".as_slice(), false))
                .ok_or_else(|| throttle(503, u64::MAX))
        });
        let engine = DownloadEngine::default();
        engine
            .download(
                &network,
                job(&destination, &[PRIMARY, SAME_ORIGIN, ALTERNATE], 2),
            )
            .await
            .unwrap();
        let error = engine
            .download(&network, job(&root.path().join("other"), &[SAME_ORIGIN], 2))
            .await
            .unwrap_err();
        assert!(error.contains("retry budget exhausted"));
        let calls = calls.into_inner();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, PRIMARY);
        assert_eq!(calls[1].0, ALTERNATE);
        assert_eq!(calls[0].1, calls[1].1);
    });
}

struct QueuedOrigin {
    gate: tokio::sync::Notify,
    calls: RefCell<Vec<(String, Instant)>>,
    first: Cell<bool>,
}

impl Network for QueuedOrigin {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.calls.borrow_mut().push((url.into(), Instant::now()));
        if url == PRIMARY && !self.first.replace(true) {
            self.gate.notified().await;
            return Err(throttle(429, 5));
        }
        Ok(b"ok".to_vec())
    }
}

#[test]
fn queued_requests_recheck_the_origin_after_admission() {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let primary = root.path().join("primary");
        let queued = root.path().join("queued");
        let other = root.path().join("other");
        let engine = DownloadEngine::new(DownloadPolicy {
            connections: NonZeroUsize::MIN,
            ..DownloadPolicy::default()
        });
        let network = QueuedOrigin {
            gate: tokio::sync::Notify::new(),
            calls: RefCell::default(),
            first: Cell::new(false),
        };
        let mut first = engine
            .download(&network, job(&primary, &[PRIMARY], 2))
            .boxed_local();
        let mut second = engine
            .download(&network, job(&queued, &[SAME_ORIGIN], 2))
            .boxed_local();
        assert!(poll!(&mut first).is_pending());
        assert!(poll!(&mut second).is_pending());
        network.gate.notify_one();
        assert!(poll!(&mut first).is_pending());
        assert!(poll!(&mut second).is_pending());
        assert_eq!(engine.stats().active_connections, 0);
        engine
            .download(&network, job(&other, &[ALTERNATE], 2))
            .await
            .unwrap();
        let (first, second) = futures::join!(first, second);
        first.unwrap();
        second.unwrap();
        let calls = network.calls.into_inner();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[1].0, ALTERNATE);
        assert_eq!(calls[1].1, calls[0].1);
        assert!(
            calls[2..]
                .iter()
                .all(|(_, time)| *time >= calls[0].1 + Duration::from_secs(5))
        );
    });
}

#[test]
fn cancelling_backoff_releases_the_destination_and_removes_staging() {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let destination = root.path().join("file");
        fs::write(&destination, b"old").unwrap();
        let network =
            Script(|_: &str, _: Option<RangeRequest<'_>>| Ok(whole(b"x".as_slice(), true)));
        let engine = DownloadEngine::default();
        let mut pending = engine
            .download(&network, job(&destination, &[PRIMARY], 3))
            .boxed_local();
        assert!(poll!(&mut pending).is_pending());
        assert_eq!(engine.stats().active_connections, 0);
        drop(pending);
        assert_eq!(fs::read(&destination).unwrap(), b"old");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        let ready =
            Script(|_: &str, _: Option<RangeRequest<'_>>| Ok(whole(b"new".as_slice(), false)));
        engine
            .download(&ready, job(&destination, &[PRIMARY], 3))
            .await
            .unwrap();
        assert_eq!(fs::read(destination).unwrap(), b"new");
    });
}

#[test]
fn a_waiting_range_yields_the_only_connection() {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let abandoned = root.path().join("abandoned");
        let ready = root.path().join("ready");
        let engine = DownloadEngine::new(DownloadPolicy {
            connections: NonZeroUsize::MIN,
            connections_per_file: NonZeroUsize::MIN,
            ..DownloadPolicy::default()
        });
        let network = Script(|_: &str, range: Option<RangeRequest<'_>>| Ok(waiting_range(range)));
        let before = Instant::now();
        let mut waiting = engine
            .download(&network, job(&abandoned, &[PRIMARY], SIZE))
            .boxed_local();
        assert!(poll!(&mut waiting).is_pending());
        assert_eq!(engine.stats().active_connections, 0);

        let independent = engine.download(&network, job(&ready, &[ALTERNATE], 2));
        tokio::time::timeout(Duration::from_millis(1), independent)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(Instant::now(), before);
        drop(waiting);
        assert_eq!(fs::read(&ready).unwrap(), b"ok");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    });
}

fn waiting_range(range: Option<RangeRequest<'_>>) -> DownloadResponse<Body> {
    let Some(range) = range else {
        return whole(b"ok".as_slice(), false);
    };
    let mut response = whole(b"a".as_slice(), true);
    response.range = Some(ResponseRange {
        interval: range.interval,
        total: SIZE,
    });
    response.representation = Some(representation(PRIMARY, Some(OLD_TAG)));
    response
}

#[test]
fn local_publication_failure_skips_other_sources() {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let destination = root.path().join("existing-directory");
        fs::create_dir(&destination).unwrap();
        let calls = Cell::new(0);
        let network = Script(|_: &str, _: Option<RangeRequest<'_>>| {
            calls.set(calls.get() + 1);
            Ok(whole(b"ok".as_slice(), false))
        });
        let error = DownloadEngine::default()
            .download(&network, job(&destination, &[PRIMARY, ALTERNATE], 2))
            .await
            .unwrap_err();
        assert!(error.contains("cannot write download"));
        assert_eq!(calls.get(), 1);
        assert!(destination.is_dir());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    });
}

#[test]
fn duplicate_destination_waiters_reuse_the_verified_file() {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let destination = root.path().join("file");
        let calls = Cell::new(0);
        let network = Script(|_: &str, _: Option<RangeRequest<'_>>| {
            calls.set(calls.get() + 1);
            Ok(whole(b"abc".as_slice(), false))
        });
        let engine = DownloadEngine::default();
        let mut file = job(&destination, &[PRIMARY], 3);
        file.sha1 = Some("a9993e364706816aba3e25717850c26c9cd0d89d");
        let (first, second) = futures::join!(
            engine.download(&network, file),
            engine.download(&network, file)
        );
        first.unwrap();
        second.unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(fs::read(destination).unwrap(), b"abc");
    });
}

#[test]
fn a_batch_reports_one_completion_after_each_file_resolves() {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let calls = Cell::new(0);
        let network = Script(|url: &str, _: Option<RangeRequest<'_>>| batch_response(url, &calls));
        let files = [PRIMARY, ALTERNATE].map(|url| MissingFile {
            url: url.into(),
            path: root.path().join(
                url.trim_start_matches("https://")
                    .split('/')
                    .next()
                    .unwrap(),
            ),
            sha1: None,
            sha1_alternatives: Vec::new(),
            size: Some(2),
        });
        let mut processed = Vec::new();
        let results = Direct(&network)
            .batch(&files, 2)
            .inspect(|_| processed.push(processed.len() + 1))
            .collect::<Vec<_>>()
            .await;
        assert_eq!(processed, [1, 2]);
        assert_eq!(results[0].path, files[1].path);
        assert_eq!(results[1].path, files[0].path);
        assert!(results.iter().all(|outcome| outcome.result.is_ok()));
        assert_eq!(calls.get(), 2);
    });
}

fn batch_response(url: &str, calls: &Cell<usize>) -> Result<DownloadResponse<Body>, FetchError> {
    if url == PRIMARY {
        calls.set(calls.get() + 1);
        if calls.get() == 1 {
            return Err(retryable());
        }
    }
    Ok(whole(b"ok".as_slice(), false))
}

#[test]
fn a_whole_file_integrity_failure_allows_exactly_one_clean_retry() {
    paused(check_whole_integrity_retry(true));
    paused(check_whole_integrity_retry(false));
}

async fn check_whole_integrity_retry(recovers: bool) {
    let root = test_tempfile::tempdir().unwrap();
    let destination = root.path().join("file");
    fs::write(&destination, b"old").unwrap();
    let calls = Cell::new(0);
    let network = Script(|_: &str, range: Option<RangeRequest<'_>>| {
        assert!(range.is_none());
        let number = calls.get() + 1;
        calls.set(number);
        let bytes = if recovers && number == 2 {
            b"abc"
        } else {
            b"bad"
        };
        Ok(whole(bytes.as_slice(), false))
    });
    let engine = DownloadEngine::default();
    let mut file = job(&destination, &[PRIMARY], 3);
    file.sha1 = Some("a9993e364706816aba3e25717850c26c9cd0d89d");
    let result = engine.download(&network, file).await;
    assert_eq!(calls.get(), 2);
    if recovers {
        result.unwrap();
        assert_eq!(fs::read(destination).unwrap(), b"abc");
    } else {
        assert!(result.unwrap_err().contains("SHA-1 mismatch"));
        assert_eq!(fs::read(destination).unwrap(), b"old");
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}
