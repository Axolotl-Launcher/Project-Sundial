// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop access to project description images.

use std::{future::Future, time::Duration};

use axlauncher_network_spi::FetchError;

const IMAGE_TIMEOUT: Duration = Duration::from_secs(15);

/// Downloads an image for a project description. Call from a background task.
pub fn project_image(url: &str) -> Result<Vec<u8>, String> {
    run_image_request(
        axlauncher_network::project_images::project_image(url),
        IMAGE_TIMEOUT,
    )
}

fn run_image_request(
    request: impl Future<Output = Result<Vec<u8>, FetchError>>,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let response = runtime.block_on(async { tokio::time::timeout(timeout, request).await });
    // Dropping the runtime can wait for an OS DNS lookup even after a timeout.
    runtime.shutdown_background();
    response
        .map_err(|_| "The project image request timed out".to_owned())?
        .map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/src/desktop/project_images.rs"]
mod tests;
