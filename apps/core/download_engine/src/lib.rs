// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Concurrent file downloads with digest verification and atomic writes.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use axlauncher_network::verify;
use axlauncher_spi::{FetchError, Network};
use axlauncher_storage::write_atomic;
use futures::stream::{self, StreamExt};

// Cargo Shear scans imports inside the crate but misses the mirrored tests.
#[cfg(test)]
use tempfile as test_tempfile;

/// The number of downloads in flight when a caller passes zero.
pub const DEFAULT_CONCURRENCY: usize = 4;

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

/// Fetches each file and checks its SHA-1 before writing if one was supplied.
///
/// At most `concurrency` requests are in flight at once; zero means
/// [`DEFAULT_CONCURRENCY`]. Outcomes arrive in completion order.
pub async fn download_all(
    network: &impl Network,
    files: &[MissingFile],
    concurrency: usize,
) -> Vec<DownloadOutcome> {
    download_all_reporting(network, files, concurrency, |_| {}).await
}

/// Downloads files like [`download_all`], reporting the count after each
/// completed attempt, including failures.
pub async fn download_all_reporting(
    network: &impl Network,
    files: &[MissingFile],
    concurrency: usize,
    mut report: impl FnMut(usize),
) -> Vec<DownloadOutcome> {
    let limit = if concurrency == 0 {
        DEFAULT_CONCURRENCY
    } else {
        concurrency
    };
    let mut processed = 0;
    stream::iter(files)
        .map(|file| download(network, file))
        .buffer_unordered(limit)
        .inspect(|_| {
            processed += 1;
            report(processed);
        })
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
        rejected @ FetchError::Rejected { .. } => rejected.to_string(),
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/download_engine/src/lib.rs"]
mod tests;
