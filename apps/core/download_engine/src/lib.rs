// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Concurrent file downloads with digest verification and atomic writes.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::LazyLock;

use axlauncher_spi::Network;
use futures::stream::{self, Stream, StreamExt};

mod engine;
mod ranges;
mod resources;
mod transfer;
mod weighted;

pub use axlauncher_spi::DownloadProtocol;
pub use engine::{DownloadEngine, DownloadJob, DownloadPolicy, TransferStats};

// Cargo Shear scans imports inside the crate but misses the mirrored tests.
#[cfg(test)]
use tempfile as test_tempfile;

/// The default batch width and shared engine connection limit.
pub const DEFAULT_CONCURRENCY: usize = 256;

static SHARED_ENGINE: LazyLock<DownloadEngine> = LazyLock::new(DownloadEngine::default);

/// The source, destination and metadata for one file to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingFile {
    pub url: String,
    pub path: PathBuf,
    pub sha1: Option<String>,
    /// The expected size used by callers when planning which files to fetch.
    pub size: Option<u64>,
}

/// What happened to one file in a download run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome {
    pub path: PathBuf,
    pub result: Result<(), String>,
}

/// Downloads files concurrently and promotes them after verification.
///
/// At most `concurrency` file jobs are in flight, with zero using
/// [`DEFAULT_CONCURRENCY`]. The engine applies its own connection limits to
/// the requests each job makes. Outcomes arrive in completion order.
pub async fn download_all(
    network: &impl Network,
    files: &[MissingFile],
    concurrency: usize,
) -> Vec<DownloadOutcome> {
    download_all_reporting(network, files, concurrency, |_| {}).await
}

/// Downloads files like [`download_all`], reporting the count after each
/// file job resolves, including failures.
pub async fn download_all_reporting(
    network: &impl Network,
    files: &[MissingFile],
    concurrency: usize,
    mut report: impl FnMut(usize),
) -> Vec<DownloadOutcome> {
    let mut processed = 0;
    download_batch(network, files, concurrency)
        .inspect(|_| {
            processed += 1;
            report(processed);
        })
        .collect()
        .await
}

/// Yields each file's outcome as it completes. Dropping the stream cancels
/// unfinished jobs and preserves files that have already been promoted.
pub fn download_batch<'a>(
    network: &'a impl Network,
    files: &'a [MissingFile],
    concurrency: usize,
) -> impl Stream<Item = DownloadOutcome> + 'a {
    let limit = if concurrency == 0 {
        DEFAULT_CONCURRENCY
    } else {
        concurrency
    };
    stream::iter(files)
        .map(|file| download(network, file))
        .buffer_unordered(limit)
}

/// The outcome of fetching and writing one file.
async fn download(network: &impl Network, file: &MissingFile) -> DownloadOutcome {
    DownloadOutcome {
        path: file.path.clone(),
        result: DownloadEngine::shared()
            .download(
                network,
                DownloadJob {
                    sources: &[&file.url],
                    destination: &file.path,
                    sha1: file.sha1.as_deref(),
                    size: file.size,
                    batch: true,
                },
            )
            .await,
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/download_engine/src/lib.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../../tests/apps/core/download_engine/src/streaming.rs"]
mod streaming;
