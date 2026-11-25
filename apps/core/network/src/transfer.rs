// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::time::Duration;

use axlauncher_spi::{ByteRange, DownloadBody, DownloadResponse, FetchError, ResponseRange};
use bytes::Bytes;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_RANGE, RANGE};
use reqwest::{Client, Response, StatusCode, Version};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const FILE_READ_TIMEOUT: Duration = Duration::from_secs(30);
const RANGE_READ_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) async fn open(
    client: &Client,
    url: &str,
    version: Option<Version>,
    range: Option<ByteRange>,
    connect_failed: impl FnOnce(),
) -> Result<DownloadResponse<impl DownloadBody>, FetchError> {
    let mut request = client.get(url).header(ACCEPT_ENCODING, "identity");
    if let Some(version) = version {
        request = request.version(version);
    }
    if let Some(range) = range {
        if range.start >= range.end {
            return Err(FetchError::Fatal("empty byte range".into()));
        }
        request = request.header(RANGE, format!("bytes={}-{}", range.start, range.end - 1));
    }
    let mut response = tokio::time::timeout(RESPONSE_TIMEOUT, request.send())
        .await
        .map_err(|_| FetchError::Retryable("download response timed out".into()))?
        .map_err(|error| {
            if error.is_connect() {
                connect_failed();
            }
            request_error(error)
        })?;
    let received = response.version();
    let compatible = match version {
        Some(Version::HTTP_11) => matches!(received, Version::HTTP_10 | Version::HTTP_11),
        Some(required) => received == required,
        None => true,
    };
    if !compatible {
        return Err(FetchError::Fatal(format!(
            "unexpected download protocol {received:?}"
        )));
    }
    let status = response.status();
    if !status.is_success() {
        let excerpt = tokio::time::timeout(RESPONSE_TIMEOUT, response.chunk())
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
            .unwrap_or_default();
        return Err(super::status_error(
            status,
            excerpt[..excerpt.len().min(4096)].to_vec(),
        ));
    }
    describe(response)
}

fn describe(response: Response) -> Result<DownloadResponse<ResponseBody>, FetchError> {
    let headers = response.headers();
    let range = if response.status() == StatusCode::PARTIAL_CONTENT {
        let range = headers
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_range);
        Some(range.ok_or_else(|| FetchError::Rejected {
            status: StatusCode::PARTIAL_CONTENT.as_u16(),
            body: b"invalid Content-Range".to_vec(),
        })?)
    } else {
        None
    };
    let length = response.content_length();
    let timeout = if range.is_some() {
        RANGE_READ_TIMEOUT
    } else {
        FILE_READ_TIMEOUT
    };
    Ok(DownloadResponse {
        length,
        range,
        body: ResponseBody { response, timeout },
    })
}

fn parse_range(value: &str) -> Option<ResponseRange> {
    let (span, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, last) = span.split_once('-')?;
    let start = start.parse().ok()?;
    let end = last.parse::<u64>().ok()?.checked_add(1)?;
    let total = total.parse().ok()?;
    (start < end && end <= total).then_some(ResponseRange {
        interval: ByteRange { start, end },
        total,
    })
}

struct ResponseBody {
    response: Response,
    timeout: Duration,
}

impl DownloadBody for ResponseBody {
    async fn next_chunk(&mut self) -> Result<Option<Bytes>, FetchError> {
        tokio::time::timeout(self.timeout, self.response.chunk())
            .await
            .map_err(|_| FetchError::Retryable("download body read timed out".into()))?
            .map_err(request_error)
    }
}

fn request_error(error: reqwest::Error) -> FetchError {
    if error.is_builder() {
        FetchError::Fatal(error.to_string())
    } else {
        FetchError::Retryable(error.to_string())
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/network/src/transfer.rs"]
mod tests;
