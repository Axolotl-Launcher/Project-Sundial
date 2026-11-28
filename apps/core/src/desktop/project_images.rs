// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop access to project description images.

use std::{future::Future, sync::OnceLock, time::Duration};

use axlauncher_network_spi::FetchError;
use tokio::{
    runtime::Runtime,
    sync::{Semaphore, oneshot},
};

const IMAGE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_ACTIVE_REQUESTS: usize = 8;
static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();
static REQUESTS: Semaphore = Semaphore::const_new(MAX_ACTIVE_REQUESTS);

/// Downloads a project description image on the shared network runtime.
pub async fn project_image(url: String) -> Result<Vec<u8>, String> {
    run_image_request(
        async move { axlauncher_network::project_images::project_image(&url).await },
        IMAGE_TIMEOUT,
    )
    .await
}

async fn run_image_request(
    request: impl Future<Output = Result<Vec<u8>, FetchError>> + Send + 'static,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let runtime = RUNTIME
        .get_or_init(image_runtime)
        .as_ref()
        .map_err(Clone::clone)?;
    let transfer = async {
        let _permit = REQUESTS
            .acquire()
            .await
            .expect("the image semaphore is never closed");
        request.await
    };
    let (mut reply, response) = oneshot::channel();
    runtime.spawn(async move {
        tokio::select! {
            biased;
            () = reply.closed() => {}
            result = tokio::time::timeout(timeout, transfer) => {
                let result = match result {
                    Ok(result) => result.map_err(|error| error.to_string()),
                    Err(_) => Err("The project image request timed out".to_owned()),
                };
                let _ = reply.send(result);
            }
        }
    });
    response
        .await
        .map_err(|_| "The project image task ended without a response".to_owned())?
}

fn image_runtime() -> Result<Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .max_blocking_threads(MAX_ACTIVE_REQUESTS)
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/src/desktop/project_images.rs"]
mod tests;
