// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! HTTP transport for authentication requests.

use std::time::Duration;

use axlauncher_spi::auth::{AuthNetwork, Body, Request, Response, TransportError};
use reqwest::header::{ACCEPT, CONTENT_TYPE, USER_AGENT};

mod callback;
pub use callback::{BrowserCallback, CallbackError};

const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BODY_BYTES: usize = 256 * 1024;

/// Sends authentication requests over HTTPS without redirecting credentials.
#[derive(Clone)]
pub struct AuthHttp(reqwest::Client);

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
            .map(Self)
            .map_err(|_| TransportError::Unavailable)
    }
}

impl AuthNetwork for AuthHttp {
    async fn send(&self, request: Request<'_>) -> Result<Response, TransportError> {
        let builder = match request.body {
            Some(Body::Form(body)) => self
                .0
                .post(request.url)
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(body),
            Some(Body::Json(body)) => self
                .0
                .post(request.url)
                .header(CONTENT_TYPE, "application/json")
                .body(body),
            None => self.0.get(request.url),
        };
        let builder = builder
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, request.user_agent);
        let builder = match request.bearer {
            Some(token) => builder.bearer_auth(token),
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
