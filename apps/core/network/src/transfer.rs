// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::time::{Duration, SystemTime};

use axlauncher_network_spi::{
    ByteRange, DownloadBody, DownloadEtag, DownloadRepresentation, DownloadResponse, FetchError,
    RangeRequest, ResponseRange,
};
use bytes::Bytes;
use reqwest::header::{
    ACCEPT_ENCODING, CONTENT_RANGE, ETAG, HeaderMap, HeaderValue, IF_RANGE, RANGE, RETRY_AFTER,
};
use reqwest::{Client, Response, StatusCode, Version};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const FILE_READ_TIMEOUT: Duration = Duration::from_secs(30);
const RANGE_READ_TIMEOUT: Duration = Duration::from_secs(15);
const REJECTION_TIMEOUT: Duration = Duration::from_millis(250);
const REJECTION_BYTES: usize = 4096;
const CDN_CONNECTION_TIMED_OUT: u16 = 522;
const CDN_ORIGIN_RESPONSE_TIMED_OUT: u16 = 524;

pub(super) async fn open(
    client: &Client,
    url: &str,
    version: Option<Version>,
    range: Option<RangeRequest<'_>>,
    key: Option<&HeaderValue>,
    connect_failed: impl FnOnce(),
) -> Result<DownloadResponse<ResponseBody>, TransferError> {
    let mut request = client.get(url).header(ACCEPT_ENCODING, "identity");
    if let Some(key) = key {
        request = request.header("x-api-key", key.clone());
    }
    if let Some(version) = version {
        request = request.version(version);
    }
    if let Some(RangeRequest { interval, if_range }) = range {
        if interval.start >= interval.end {
            return Err(FetchError::Fatal("empty byte range".into()).into());
        }
        request = request.header(
            RANGE,
            format!("bytes={}-{}", interval.start, interval.end - 1),
        );
        if let Some(tag) = if_range {
            request = request.header(IF_RANGE, tag);
        }
    }
    let response = tokio::time::timeout(RESPONSE_TIMEOUT, request.send())
        .await
        .map_err(|_| TransferError::Timeout("download response timed out".into()))?
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
        return Err(FetchError::Fatal(format!("unexpected download protocol {received:?}")).into());
    }
    Ok(describe(response, key.is_some()).await?)
}

async fn describe(
    response: Response,
    keyed: bool,
) -> Result<DownloadResponse<ResponseBody>, FetchError> {
    if !matches!(
        response.status(),
        StatusCode::OK | StatusCode::PARTIAL_CONTENT
    ) {
        return Err(download_error(response, keyed).await);
    }
    let headers = response.headers();
    let range = if response.status() == StatusCode::PARTIAL_CONTENT {
        let range = headers
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_range);
        Some(range.ok_or_else(|| FetchError::Rejected {
            status: StatusCode::PARTIAL_CONTENT.as_u16(),
            body: Vec::new(),
        })?)
    } else {
        None
    };
    let length = response.content_length();
    let representation = range.map(|_| {
        let mut resource = response.url().clone();
        resource.set_fragment(None);
        DownloadRepresentation {
            resource: resource.into(),
            etag: response_tag(headers),
        }
    });
    let timeout = if range.is_some() {
        RANGE_READ_TIMEOUT
    } else {
        FILE_READ_TIMEOUT
    };
    Ok(DownloadResponse {
        length,
        range,
        representation,
        body: ResponseBody { response, timeout },
    })
}

async fn download_error(mut response: Response, keyed: bool) -> FetchError {
    let status = response.status();
    if is_transient_status(status) {
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| retry_delay(value, SystemTime::now()));
        return FetchError::HttpRetryable {
            status: status.as_u16(),
            retry_after,
        };
    }
    FetchError::Rejected {
        status: status.as_u16(),
        body: if keyed {
            Vec::new()
        } else {
            rejection_excerpt(&mut response).await
        },
    }
}

fn is_transient_status(status: StatusCode) -> bool {
    // Include the CDN status codes used for transient upstream timeouts.
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    ) || matches!(
        status.as_u16(),
        CDN_CONNECTION_TIMED_OUT | CDN_ORIGIN_RESPONSE_TIMED_OUT
    )
}

async fn rejection_excerpt(response: &mut Response) -> Vec<u8> {
    let mut excerpt = Vec::new();
    let _ = tokio::time::timeout(REJECTION_TIMEOUT, read_excerpt(response, &mut excerpt)).await;
    excerpt
}

async fn read_excerpt(response: &mut Response, excerpt: &mut Vec<u8>) {
    while excerpt.len() < REJECTION_BYTES
        && let Ok(Some(chunk)) = response.chunk().await
    {
        excerpt.extend_from_slice(&chunk[..chunk.len().min(REJECTION_BYTES - excerpt.len())]);
    }
}

pub(super) fn retry_delay(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(Duration::from_secs(value.parse().unwrap_or(u64::MAX)));
    }
    let deadline = httpdate::parse_http_date(value).ok()?;
    Some(deadline.duration_since(now).unwrap_or_default())
}

fn entity_tag(value: &HeaderValue) -> DownloadEtag {
    let bytes = value.as_bytes();
    let quoted = bytes
        .strip_prefix(b"\"")
        .and_then(|tag| tag.strip_suffix(b"\""));
    if quoted.is_some_and(|tag| {
        tag.iter()
            .all(|byte| matches!(byte, 0x21 | 0x23..=0x7e | 0x80..=0xff))
    }) {
        return DownloadEtag::Strong(Bytes::copy_from_slice(bytes));
    }
    DownloadEtag::Unusable
}

fn response_tag(headers: &HeaderMap) -> Option<DownloadEtag> {
    let values = headers.get_all(ETAG);
    let mut values = values.iter();
    let first = values.next()?;
    if values.next().is_some() {
        return Some(DownloadEtag::Unusable);
    }
    Some(entity_tag(first))
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

pub(super) struct ResponseBody {
    response: Response,
    timeout: Duration,
}

impl DownloadBody for ResponseBody {
    async fn next_chunk(&mut self) -> Result<Option<Bytes>, FetchError> {
        Ok(self.read().await?)
    }
}

impl ResponseBody {
    pub(super) async fn read(&mut self) -> Result<Option<Bytes>, TransferError> {
        tokio::time::timeout(self.timeout, self.response.chunk())
            .await
            .map_err(|_| TransferError::Timeout("download body read timed out".into()))?
            .map_err(request_error)
    }
}

pub(super) enum TransferError {
    Fetch(FetchError),
    Redirect(String),
    Timeout(String),
}

impl From<FetchError> for TransferError {
    fn from(error: FetchError) -> Self {
        Self::Fetch(error)
    }
}

impl From<TransferError> for FetchError {
    fn from(error: TransferError) -> Self {
        match error {
            TransferError::Fetch(error) => error,
            TransferError::Redirect(detail) => Self::Fatal(detail),
            TransferError::Timeout(detail) => Self::Retryable(detail),
        }
    }
}

fn request_error(error: reqwest::Error) -> TransferError {
    use std::error::Error as _;
    use std::fmt::Write as _;

    let error = error.without_url();
    let mut detail = error.to_string();
    let mut cause = error.source();
    while let Some(error) = cause {
        let _ = write!(detail, ": {error}");
        cause = error.source();
    }
    if error.is_redirect() {
        TransferError::Redirect(detail)
    } else if error.is_timeout() {
        TransferError::Timeout(detail)
    } else if error.is_builder() {
        FetchError::Fatal(detail).into()
    } else {
        FetchError::Retryable(detail).into()
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/network/src/transfer.rs"]
mod tests;
