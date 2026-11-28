//! Bounded JSON transport for public catalog requests.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use reqwest::header::{ACCEPT, HeaderMap, RETRY_AFTER};
use reqwest::{Client, Response, StatusCode};
use url::Url;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_COOLDOWN_SECONDS: u32 = 60;

/// A metadata client that does not redirect, use mirrors, or send credentials.
pub struct MetadataHttp(Client);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataError {
    Http(u16),
    Timeout,
    Transport,
    BodyTooLarge,
}

impl MetadataHttp {
    pub fn desktop(user_agent: &str) -> Result<Self, MetadataError> {
        let resolver = super::dns::DohResolver::new(super::DohConfig::default())
            .map_err(|_| MetadataError::Transport)?;
        Client::builder()
            .dns_resolver(Arc::new(resolver))
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(user_agent)
            .build()
            .map(Self)
            .map_err(|_| MetadataError::Transport)
    }

    /// Supplies a preconfigured HTTP client, for example for loopback tests.
    pub fn new(client: Client) -> Self {
        Self(client)
    }

    /// An exhausted rate window can accompany an unsuccessful or incomplete response.
    pub async fn get(
        &self,
        url: Url,
    ) -> Result<(Option<u32>, Result<Vec<u8>, MetadataError>), MetadataError> {
        let response = self
            .0
            .get(url)
            .header(ACCEPT, "application/json")
            .send()
            .await
            .map_err(request_error)?;
        let status = response.status();
        let cooldown = cooldown_seconds(status, response.headers());
        let result = if status.is_success() {
            read_body(response).await
        } else {
            Err(MetadataError::Http(status.as_u16()))
        };
        Ok((cooldown, result))
    }
}

fn request_error(error: reqwest::Error) -> MetadataError {
    if error.is_timeout() {
        MetadataError::Timeout
    } else {
        MetadataError::Transport
    }
}

fn cooldown_seconds(status: StatusCode, headers: &HeaderMap) -> Option<u32> {
    let number = |name: &str| headers.get(name)?.to_str().ok()?.parse::<u32>().ok();
    if status != StatusCode::TOO_MANY_REQUESTS && number("x-ratelimit-remaining") != Some(0) {
        return None;
    }
    let retry_after = headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| super::transfer::retry_delay(value, SystemTime::now()))
        .map(|delay| {
            let seconds = delay
                .as_secs()
                .saturating_add(u64::from(delay.subsec_nanos() != 0));
            u32::try_from(seconds).unwrap_or(u32::MAX)
        });
    Some(
        retry_after
            .into_iter()
            .chain(number("x-ratelimit-reset"))
            .max()
            .unwrap_or(DEFAULT_COOLDOWN_SECONDS)
            .max(1),
    )
}

async fn read_body(mut response: Response) -> Result<Vec<u8>, MetadataError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BODY_BYTES as u64)
    {
        return Err(MetadataError::BodyTooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(request_error)? {
        if chunk.len() > MAX_BODY_BYTES - body.len() {
            return Err(MetadataError::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
