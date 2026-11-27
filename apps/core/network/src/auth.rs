// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! HTTP transport for authentication requests.

use std::time::Duration;

use axlauncher_network_spi::ServiceHost;
use axlauncher_network_spi::auth::{AuthNetwork, Body, Request, Response, TransportError};
use reqwest::header::{ACCEPT, CONTENT_TYPE, USER_AGENT};
use url::Url;

mod callback;
pub use callback::{BrowserCallback, CallbackError};

const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BODY_BYTES: usize = 256 * 1024;

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
                super::api_key_header(key).map_err(|_| TransportError::InvalidResponse)?,
            ),
            None => builder,
        };
        read_response(
            builder
                .send()
                .await
                .map_err(|_| TransportError::Unavailable)?,
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

async fn read_response(mut response: reqwest::Response) -> Result<Response, TransportError> {
    let status = response.status().as_u16();
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| TransportError::Unavailable)?
    {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(TransportError::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Response { status, body })
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/network/src/auth.rs"]
mod tests;
