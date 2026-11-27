// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::cell::{Cell, RefCell};
use std::fs;
use std::num::NonZeroUsize;
use std::time::Duration;

use axlauncher_network_spi::{
    ByteRange, DownloadBody, DownloadProtocol, DownloadResponse, FetchError, Network, RangeRequest,
    ResponseRange,
};
use bytes::Bytes;
use tokio::time::Instant;

use super::range_identity::representation;
use super::retry_tests::{ALTERNATE, Body, OLD_TAG, PRIMARY, Script, job, paused, whole};
use super::{DownloadEngine, DownloadPolicy, test_tempfile};
use crate::weighted::MIN_RANGE_BYTES;

#[test]
fn four_adjacent_failed_ranges_share_a_single_recovery() {
    paused(check_simultaneous_failures(4, false));
}

#[test]
fn four_disjoint_failures_use_only_three_recoveries_before_switching_source() {
    paused(check_simultaneous_failures(8, true));
}

async fn check_simultaneous_failures(width: usize, disjoint: bool) {
    let root = test_tempfile::tempdir().unwrap();
    let destination = root.path().join("file");
    let total = width as u64 * MIN_RANGE_BYTES;
    let calls = RefCell::new(Vec::new());
    let network = Script(|url: &str, range: Option<RangeRequest<'_>>| {
        let range = range.expect("ranged recovery");
        let mut calls = calls.borrow_mut();
        let initial = calls.len() < width;
        calls.push((url == PRIMARY, range.interval));
        if url == ALTERNATE {
            return Ok(whole(vec![b'a'; total as usize], false));
        }
        let failed =
            initial && (!disjoint || (range.interval.start / MIN_RANGE_BYTES).is_multiple_of(2));
        Ok(partial(range, total, failed))
    });
    let engine = DownloadEngine::new(DownloadPolicy {
        connections_per_file: NonZeroUsize::new(width).unwrap(),
        ..DownloadPolicy::default()
    });
    let sources = [PRIMARY, ALTERNATE];
    engine
        .download(&network, job(&destination, &sources, total))
        .await
        .unwrap();
    let calls = calls.into_inner();
    let recoveries = &calls[width..];
    if disjoint {
        assert_eq!(recoveries.len(), 4);
        assert!(
            recoveries[..3]
                .iter()
                .all(|(primary, range)| *primary && range.end - range.start == MIN_RANGE_BYTES)
        );
        assert!(!recoveries[3].0);
    } else {
        assert_eq!(
            recoveries,
            [(
                true,
                ByteRange {
                    start: 0,
                    end: total
                }
            )]
        );
    }
    assert_eq!(fs::read(destination).unwrap(), vec![b'a'; total as usize]);
}

#[test]
fn throttled_range_resumes_after_cooldown_without_reopening_the_first_range() {
    paused(check_throttled_range(false));
}

#[test]
fn a_ready_alternate_wins_over_a_throttled_range() {
    paused(check_throttled_range(true));
}

async fn check_throttled_range(use_alternate: bool) {
    let root = test_tempfile::tempdir().unwrap();
    let destination = root.path().join("file");
    let total = 2 * MIN_RANGE_BYTES;
    let calls = RefCell::new(Vec::new());
    let throttled = Cell::new(false);
    let network = Script(|url: &str, range: Option<RangeRequest<'_>>| {
        let range = range.expect("stay in range mode");
        calls
            .borrow_mut()
            .push((url == PRIMARY, range.interval.start, Instant::now()));
        if url == ALTERNATE {
            return Ok(whole(vec![b'a'; total as usize], false));
        }
        if range.interval.start != 0 && !throttled.replace(true) {
            return Err(FetchError::HttpRetryable {
                status: 429,
                retry_after: Some(Duration::from_secs(5)),
            });
        }
        Ok(partial(range, total, false))
    });
    let engine = DownloadEngine::default();
    let sources = [PRIMARY, ALTERNATE];
    engine
        .download(
            &network,
            job(
                &destination,
                &sources[..1 + usize::from(use_alternate)],
                total,
            ),
        )
        .await
        .unwrap();
    let calls = calls.into_inner();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].1, 0);
    assert_eq!(calls[1].1, MIN_RANGE_BYTES);
    if use_alternate {
        assert_eq!(calls[2], (false, 0, calls[1].2));
    } else {
        assert_eq!((calls[2].0, calls[2].1), (true, MIN_RANGE_BYTES));
        assert!(calls[2].2 >= calls[1].2 + Duration::from_secs(5));
    }
    assert_eq!(fs::read(destination).unwrap(), vec![b'a'; total as usize]);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

fn partial(range: RangeRequest<'_>, total: u64, fail: bool) -> DownloadResponse<Body> {
    let interval = range.interval;
    let len = if fail {
        0
    } else {
        interval.end - interval.start
    };
    let mut response = whole(vec![b'a'; len as usize], fail);
    response.range = Some(ResponseRange { interval, total });
    response.representation = Some(representation(PRIMARY, Some(OLD_TAG)));
    response
}

#[test]
fn throttling_cancels_an_active_body_and_keeps_its_written_prefix() {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let destination = root.path().join("file");
        let network = StalledRange {
            calls: RefCell::new(Vec::new()),
        };
        let engine = DownloadEngine::default();
        let total = 2 * MIN_RANGE_BYTES;
        let sources = [PRIMARY];
        let task = engine.download(&network, job(&destination, &sources, total));
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(network.calls.into_inner(), [0, MIN_RANGE_BYTES, 1]);
        assert_eq!(engine.stats().bytes_received, total);
        assert_eq!(engine.stats().active_connections, 0);
        assert_eq!(fs::read(destination).unwrap(), vec![b'a'; total as usize]);
    });
}

struct StalledRange {
    calls: RefCell<Vec<u64>>,
}

impl Network for StalledRange {
    async fn fetch(&self, _: &str) -> Result<Vec<u8>, FetchError> {
        panic!("expected a range")
    }

    async fn open_download(
        &self,
        _: &str,
        _: DownloadProtocol,
        range: Option<RangeRequest<'_>>,
    ) -> Result<DownloadResponse<impl DownloadBody>, FetchError> {
        let interval = range.unwrap().interval;
        self.calls.borrow_mut().push(interval.start);
        let total = 2 * MIN_RANGE_BYTES;
        if interval.start == MIN_RANGE_BYTES {
            return Err(FetchError::HttpRetryable {
                status: 429,
                retry_after: Some(Duration::from_secs(5)),
            });
        }
        let stalls = interval.start == 0;
        let length = if stalls {
            1
        } else {
            interval.end - interval.start
        };
        Ok(DownloadResponse {
            range: Some(ResponseRange { interval, total }),
            length: None,
            representation: Some(representation(PRIMARY, Some(OLD_TAG))),
            body: StallingBody {
                chunk: Some(Bytes::from(vec![b'a'; length as usize])),
                stalls,
            },
        })
    }
}

struct StallingBody {
    chunk: Option<Bytes>,
    stalls: bool,
}

impl DownloadBody for StallingBody {
    async fn next_chunk(&mut self) -> Result<Option<Bytes>, FetchError> {
        let chunk = self.chunk.take();
        if chunk.is_none() && self.stalls {
            return std::future::pending().await;
        }
        Ok(chunk)
    }
}
