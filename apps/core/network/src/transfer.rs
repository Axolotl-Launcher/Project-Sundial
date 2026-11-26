// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::time::{Duration, SystemTime};

use axlauncher_spi::{
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

pub(super) async fn open(
    client: &Client,
    url: &str,
    version: Option<Version>,
    range: Option<RangeRequest<'_>>,
    key: Option<&HeaderValue>,
    connect_failed: impl FnOnce(),
) -> Result<DownloadResponse<impl DownloadBody>, FetchError> {
    let mut request = client.get(url).header(ACCEPT_ENCODING, "identity");
    if let Some(key) = key {
        request = request.header("x-api-key", key.clone());
    }
    if let Some(version) = version {
        request = request.version(version);
    }
    if let Some(RangeRequest { interval, if_range }) = range {
        if interval.start >= interval.end {
            return Err(FetchError::Fatal("empty byte range".into()));
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
    describe(response)
}

fn describe(response: Response) -> Result<DownloadResponse<ResponseBody>, FetchError> {
    if !matches!(
        response.status(),
        StatusCode::OK | StatusCode::PARTIAL_CONTENT
    ) {
        return Err(download_status(&response));
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

fn download_status(response: &Response) -> FetchError {
    let status = response.status().as_u16();
    if matches!(status, 408 | 429 | 500 | 502 | 503 | 504 | 522 | 524) {
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| retry_delay(value, SystemTime::now()));
        return FetchError::HttpRetryable {
            status,
            retry_after,
        };
    }
    FetchError::Rejected {
        status,
        body: Vec::new(),
    }
}

fn retry_delay(value: &str, now: SystemTime) -> Option<Duration> {
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
    if error.is_builder() || error.is_redirect() {
        FetchError::Fatal(error.without_url().to_string())
    } else {
        FetchError::Retryable(error.without_url().to_string())
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/network/src/transfer.rs"]
mod tests;
