// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Bounded retrieval of remote images embedded in project descriptions.

use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;

use axlauncher_network_spi::FetchError;
use reqwest::Client;
use url::Url;

use crate::{redirect_policy, transfer};
use transfer::TransferError;

const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
// Applies to establishing a connection. The desktop gateway sets the full request deadline.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
// Reuse a few connections to each CDN without retaining the full request concurrency when idle.
const MAX_IDLE_CONNECTIONS_PER_HOST: usize = 4;
static CLIENT: OnceLock<Result<Client, ProjectImageError>> = OnceLock::new();

/// A failure while retrieving an image for a project description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectImageError {
    InvalidUrl,
    Redirect(String),
    Timeout(String),
    TooLarge,
    TemporaryTransport(String),
    TemporaryHttp {
        status: u16,
        retry_after: Option<Duration>,
    },
    HttpRejected(u16),
    Internal(String),
}

impl ProjectImageError {
    /// Transient failures permit retry after this delay. Permanent failures yield `None`.
    pub fn retry_delay(&self) -> Option<Duration> {
        match self {
            Self::Timeout(_) | Self::TemporaryTransport(_) => Some(Duration::ZERO),
            Self::TemporaryHttp { retry_after, .. } => Some(retry_after.unwrap_or_default()),
            _ => None,
        }
    }
}

impl fmt::Display for ProjectImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl => formatter.write_str("Invalid project image URL"),
            Self::Redirect(detail) => write!(formatter, "Project image redirect failed: {detail}"),
            Self::Timeout(detail) => write!(formatter, "Project image timed out: {detail}"),
            Self::TooLarge => formatter.write_str("Project image exceeds the response size limit"),
            Self::TemporaryTransport(detail) => {
                write!(formatter, "Temporary transport error: {detail}")
            }
            Self::TemporaryHttp {
                status,
                retry_after: Some(delay),
            } => {
                write!(formatter, "Temporary HTTP {status} (retry after {delay:?})")
            }
            Self::TemporaryHttp {
                status,
                retry_after: None,
            } => {
                write!(formatter, "Temporary HTTP {status}")
            }
            Self::HttpRejected(status) => write!(formatter, "HTTP {status}"),
            Self::Internal(detail) => write!(formatter, "Internal image request error: {detail}"),
        }
    }
}

impl std::error::Error for ProjectImageError {}

impl From<TransferError> for ProjectImageError {
    fn from(error: TransferError) -> Self {
        match error {
            TransferError::Redirect(detail) => Self::Redirect(detail),
            TransferError::Timeout(detail) => Self::Timeout(detail),
            TransferError::Fetch(FetchError::Retryable(detail)) => Self::TemporaryTransport(detail),
            TransferError::Fetch(FetchError::HttpRetryable {
                status,
                retry_after,
            }) => Self::TemporaryHttp {
                status,
                retry_after,
            },
            TransferError::Fetch(FetchError::Rejected { status, .. }) => Self::HttpRejected(status),
            TransferError::Fetch(FetchError::Fatal(detail)) => Self::Internal(detail),
        }
    }
}

fn allows_image(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
}

/// Reads a bounded response and validates every request URL, including redirects.
pub async fn project_image(url: &str) -> Result<Vec<u8>, ProjectImageError> {
    if !Url::parse(url).is_ok_and(|url| allows_image(&url)) {
        return Err(ProjectImageError::InvalidUrl);
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
        return Err(ProjectImageError::TooLarge);
    }

    let mut body = response.body;
    let mut bytes = Vec::new();
    while let Some(chunk) = body.read().await? {
        if chunk.len() > MAX_IMAGE_BYTES - bytes.len() {
            return Err(ProjectImageError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn image_client() -> Result<Client, ProjectImageError> {
    // External image hosts follow the configured system resolver and proxy.
    Client::builder()
        .user_agent(concat!("Axolotl-Launcher/", env!("CARGO_PKG_VERSION")))
        .redirect(redirect_policy(Some(allows_image), None))
        .connect_timeout(CONNECT_TIMEOUT)
        .pool_max_idle_per_host(MAX_IDLE_CONNECTIONS_PER_HOST)
        .build()
        .map_err(|error| ProjectImageError::Internal(error.without_url().to_string()))
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/network/src/project_images.rs"]
mod tests;
