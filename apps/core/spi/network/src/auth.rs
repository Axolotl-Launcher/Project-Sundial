// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Transport for credential-bearing HTTP requests.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

/// Sends requests whose status and response body the caller must interpret.
pub trait AuthNetwork {
    fn send(&self, request: Request<'_>) -> impl Future<Output = Result<Response, TransportError>>;
}

/// A validated header secret. Only the transport should expose its value.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(Arc<str>);

impl ApiKey {
    pub fn new(value: &str) -> Option<Self> {
        (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_graphic()))
            .then(|| Self(Arc::from(value)))
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiKey([redacted])")
    }
}

/// The shared request fields for token exchanges and authenticated reads.
#[non_exhaustive]
pub struct Request<'a> {
    pub url: &'a str,
    pub user_agent: &'a str,
    pub bearer: Option<&'a str>,
    pub body: Option<Body>,
    api_key: Option<&'a ApiKey>,
    minecraft_service: bool,
}

impl<'a> Request<'a> {
    pub fn new(url: &'a str, user_agent: &'a str) -> Self {
        Self {
            url,
            user_agent,
            bearer: None,
            body: None,
            api_key: None,
            minecraft_service: false,
        }
    }

    pub fn with_bearer(mut self, token: &'a str) -> Self {
        self.bearer = Some(token);
        self
    }

    pub fn with_body(mut self, body: Body) -> Self {
        self.body = Some(body);
        self
    }

    pub fn with_api_key(mut self, key: &'a ApiKey) -> Self {
        self.api_key = Some(key);
        self
    }

    pub fn api_key(&self) -> Option<&'a ApiKey> {
        self.api_key
    }

    /// Marks an official Minecraft services request as eligible for forwarding.
    pub fn with_minecraft_service(mut self) -> Self {
        self.minecraft_service = true;
        self
    }

    pub fn is_minecraft_service(&self) -> bool {
        self.minecraft_service
    }
}

/// An encoded request body for the supported authentication content types.
pub enum Body {
    Form(String),
    Json(String),
}

/// A bounded response whose body may contain credentials.
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub retry_after_seconds: Option<u32>,
}

impl Response {
    pub fn new(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            body,
            retry_after_seconds: None,
        }
    }
}

/// A transport failure with no secret-bearing request or response details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    Unavailable,
    Timeout,
    InvalidResponse,
}
