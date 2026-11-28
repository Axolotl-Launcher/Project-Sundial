// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Bounded retrieval of remote images embedded in project descriptions.

use std::sync::OnceLock;
use std::time::Duration;

use axlauncher_network_spi::{DownloadBody, FetchError};
use reqwest::Client;
use url::Url;

use crate::{redirect_policy, transfer};

const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
static CLIENT: OnceLock<Result<Client, FetchError>> = OnceLock::new();

fn allows_image(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
}

/// Reads a bounded response and validates every request URL, including redirects.
pub async fn project_image(url: &str) -> Result<Vec<u8>, FetchError> {
    if !Url::parse(url).is_ok_and(|url| allows_image(&url)) {
        return Err(FetchError::Fatal("Invalid project image URL".into()));
    }
    let client = CLIENT
        .get_or_init(image_client)
        .as_ref()
        .map_err(Clone::clone)?;
    let response = transfer::open(client, url, None, None, None, || {}).await?;
    if response
        .length
        .is_some_and(|size| size > MAX_IMAGE_BYTES as u64)
    {
        return Err(image_too_large());
    }

    let mut body = response.body;
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next_chunk().await? {
        if chunk.len() > MAX_IMAGE_BYTES - bytes.len() {
            return Err(image_too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn image_client() -> Result<Client, FetchError> {
    // External image hosts follow the configured system resolver and proxy.
    Client::builder()
        .user_agent(concat!("Axolotl-Launcher/", env!("CARGO_PKG_VERSION")))
        .redirect(redirect_policy(Some(allows_image), None))
        .connect_timeout(Duration::from_secs(15))
        .pool_max_idle_per_host(4)
        .build()
        .map_err(|error| FetchError::Fatal(error.without_url().to_string()))
}

fn image_too_large() -> FetchError {
    FetchError::Fatal("Project image exceeds the preview size limit".into())
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/network/src/project_images.rs"]
mod tests;
