// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Concurrent file downloads with digest verification and atomic writes.

use axlauncher_spi::{
    Network,
    downloads::{ExtraDigest, FileDownloader},
};

mod engine;
mod ranges;
mod resources;
mod retry;
mod transfer;
mod verify;
mod weighted;
#[cfg(feature = "worker")]
mod worker;

pub use axlauncher_spi::DownloadProtocol;
pub use axlauncher_spi::downloads::{DEFAULT_CONCURRENCY, DownloadJob, TransferStats};
pub use engine::{DownloadEngine, DownloadPolicy};

// Cargo Shear scans imports inside the crate but misses the mirrored tests.
#[cfg(test)]
use tempfile as test_tempfile;

/// In-process connection of a transfer engine and a network provider.
pub struct Direct<'a, Transport>(pub &'a Transport);

impl<Transport: Network> FileDownloader for Direct<'_, Transport> {
    async fn download(
        &self,
        job: DownloadJob<'_>,
        digest: Option<ExtraDigest>,
    ) -> Result<(), String> {
        let engine = DownloadEngine::shared();
        match digest {
            Some(digest) => {
                engine
                    .download_verified(self.0, job, move |file| digest.verify(file))
                    .await
            }
            None => engine.download(self.0, job).await,
        }
    }
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/download_engine/src/lib.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/download_engine/src/streaming.rs"]
mod streaming;

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/download_engine/src/lib/retry_tests.rs"]
mod retry_tests;

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/download_engine/src/lib/range_identity.rs"]
mod range_identity;

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/download_engine/src/lib/range_recovery.rs"]
mod range_recovery;
