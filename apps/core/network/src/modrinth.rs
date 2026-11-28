// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! In-memory retrieval of images served by the Modrinth CDN.

use axlauncher_network_spi::{DownloadBody, DownloadProtocol, FetchError, Network};
use url::Url;

use crate::ReqwestNetwork;

const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

/// Project images come from the Modrinth CDN's data path over HTTPS.
pub fn is_project_image_url(url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| allows_image(&url))
}

fn allows_image(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("cdn.modrinth.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path().starts_with("/data/")
}

/// Reads a bounded response from the CDN and applies the same rule to redirects.
pub async fn project_image(url: &str) -> Result<Vec<u8>, FetchError> {
    let client = ReqwestNetwork::desktop_with_download_policy(allows_image, None)?;
    let response = client
        .open_download(url, DownloadProtocol::Automatic, None)
        .await?;
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

fn image_too_large() -> FetchError {
    FetchError::Fatal("Modrinth project image exceeds the preview size limit".into())
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/network/src/modrinth.rs"]
mod tests;
