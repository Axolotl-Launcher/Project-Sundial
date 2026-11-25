// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Transport for credential-bearing HTTP requests and saved refresh tokens.

use std::future::Future;

/// Sends requests whose status and response body the caller must interpret.
pub trait AuthNetwork {
    fn send(&self, request: Request<'_>) -> impl Future<Output = Result<Response, TransportError>>;
}

/// The shared request fields for token exchanges and authenticated reads.
#[non_exhaustive]
pub struct Request<'a> {
    pub url: &'a str,
    pub user_agent: &'a str,
    pub bearer: Option<&'a str>,
    pub body: Option<Body>,
    minecraft_service: bool,
}

impl<'a> Request<'a> {
    pub fn new(url: &'a str, user_agent: &'a str) -> Self {
        Self {
            url,
            user_agent,
            bearer: None,
            body: None,
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
}

/// A transport failure with no secret-bearing request or response details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    Unavailable,
    InvalidResponse,
}

/// Stores a refresh token under an opaque launcher-generated key.
pub trait CredentialStore {
    fn read(&self, key: &str) -> Result<Option<String>, CredentialError>;
    fn write(&self, key: &str, secret: &str) -> Result<(), CredentialError>;
    fn remove(&self, key: &str) -> Result<(), CredentialError>;
}

/// The system credential store could not complete the requested operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialError;
