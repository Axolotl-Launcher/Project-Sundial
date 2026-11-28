// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! HTTP transport for authentication requests.

use std::time::{Duration, SystemTime};

use axlauncher_network_spi::ServiceHost;
use axlauncher_network_spi::auth::{AuthNetwork, Body, Request, Response, TransportError};
use reqwest::header::{ACCEPT, CONTENT_TYPE, RETRY_AFTER, USER_AGENT};
use url::Url;

mod callback;
pub use callback::{BrowserCallback, CallbackError};

const TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BODY_BYTES: usize = 256 * 1024;
const MAX_KEYED_BODY_BYTES: usize = 8 * 1024 * 1024;

/// Sends authentication requests over HTTPS without redirecting credentials.
#[derive(Clone)]
pub struct AuthHttp {
    client: reqwest::Client,
    fallen_proxy: Option<ServiceHost>,
}

impl AuthHttp {
    pub fn desktop() -> Result<Self, TransportError> {
        let resolver = super::dns::DohResolver::new(super::DohConfig::default())
            .map_err(|_| TransportError::Unavailable)?;
        reqwest::Client::builder()
            .dns_resolver(std::sync::Arc::new(resolver))
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(TIMEOUT)
            .build()
            .map(|client| Self {
                client,
                fallen_proxy: None,
            })
            .map_err(|_| TransportError::Unavailable)
    }

    /// Enables forwarding for the launcher's Minecraft services requests.
    pub fn with_fallen_proxy(mut self, host: Option<ServiceHost>) -> Self {
        self.fallen_proxy = host;
        self
    }

    async fn send_to(&self, url: &str, request: Request<'_>) -> Result<Response, TransportError> {
        let api_key = request.api_key();
        let builder = match request.body {
            Some(Body::Form(body)) => self
                .client
                .post(url)
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(body),
            Some(Body::Json(body)) => self
                .client
                .post(url)
                .header(CONTENT_TYPE, "application/json")
                .body(body),
            None => self.client.get(url),
        };
        let builder = builder
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, request.user_agent);
        let builder = match request.bearer {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        };
        let builder = match api_key {
            Some(key) => builder.header(
                "x-api-key",
                super::api_key_header(key.expose_secret())
                    .map_err(|_| TransportError::InvalidResponse)?,
            ),
            None => builder,
        };
        read_response(
            builder.send().await.map_err(request_error)?,
            api_key.is_some(),
        )
        .await
    }
}

impl AuthNetwork for AuthHttp {
    async fn send(&self, request: Request<'_>) -> Result<Response, TransportError> {
        if let Some(host) = &self.fallen_proxy
            && let Some(url) = fallen_service_url(&request, host)
        {
            return self.send_to(url.as_str(), request).await;
        }
        self.send_to(request.url, request).await
    }
}

fn fallen_service_url(request: &Request<'_>, host: &ServiceHost) -> Option<Url> {
    if !request.is_minecraft_service() || request.api_key().is_some() {
        return None;
    }
    let mut url = Url::parse(request.url).ok()?;
    let supported = matches!(
        (url.path(), &request.body),
        ("/launcher/login", Some(Body::Json(_)))
            | ("/entitlements/license" | "/minecraft/profile", None)
    );
    if !supported
        || url.scheme() != "https"
        || url.host_str() != Some("api.minecraftservices.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    url.set_host(Some(&format!("services.{}", host.as_str())))
        .ok()?;
    Some(url)
}

fn request_error(error: reqwest::Error) -> TransportError {
    if error.is_timeout() {
        TransportError::Timeout
    } else {
        TransportError::Unavailable
    }
}

async fn read_response(
    mut response: reqwest::Response,
    keyed: bool,
) -> Result<Response, TransportError> {
    let status = response.status();
    let mut reply = Response::new(status.as_u16(), Vec::new());
    if keyed && !status.is_success() {
        reply.retry_after_seconds = retry_after_seconds(response.headers());
        return Ok(reply);
    }
    let limit = if keyed {
        MAX_KEYED_BODY_BYTES
    } else {
        MAX_BODY_BYTES
    };
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(TransportError::InvalidResponse);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(request_error)? {
        if chunk.len() > limit - body.len() {
            return Err(TransportError::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    reply.body = body;
    Ok(reply)
}

fn retry_after_seconds(headers: &reqwest::header::HeaderMap) -> Option<u32> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
    let delay = super::transfer::retry_delay(value, SystemTime::now())?;
    let seconds = delay.as_secs() + u64::from(delay.subsec_nanos() != 0);
    Some(u32::try_from(seconds).unwrap_or(u32::MAX))
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/network/src/auth.rs"]
mod tests;
