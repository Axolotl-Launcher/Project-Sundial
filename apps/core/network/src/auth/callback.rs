// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Receives a browser authorization code at a registered loopback address.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Mutex;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

const TIMEOUT: Duration = Duration::from_secs(600);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3);

/// Why a browser authorization callback did not yield a code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackError {
    Unavailable,
    TimedOut,
    Denied,
}

/// A one-use listener bound exclusively to IPv4 loopback.
pub struct BrowserCallback {
    listener: TcpListener,
    path: String,
}

impl BrowserCallback {
    pub async fn bind(port: u16, path: &str) -> Result<Self, CallbackError> {
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        let listener = TcpListener::bind(address)
            .await
            .map_err(|_| CallbackError::Unavailable)?;
        Ok(Self {
            listener,
            path: path.to_owned(),
        })
    }

    pub async fn receive(self, state: &str) -> Result<String, CallbackError> {
        timeout(TIMEOUT, self.listen(state))
            .await
            .map_err(|_| CallbackError::TimedOut)?
    }

    async fn listen(self, state: &str) -> Result<String, CallbackError> {
        let mut reply = Mutex::new(None);
        loop {
            let (socket, _) = self
                .listener
                .accept()
                .await
                .map_err(|_| CallbackError::Unavailable)?;
            serve(socket, &self.path, state, &reply).await;
            match reply.get_mut().expect("callback result lock").take() {
                Some(result) => return result,
                None => continue,
            }
        }
    }
}

async fn serve(
    socket: TcpStream,
    path: &str,
    state: &str,
    reply: &Mutex<Option<Result<String, CallbackError>>>,
) {
    let connection = hyper::server::conn::http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(CONNECTION_TIMEOUT)
        .keep_alive(false)
        .serve_connection(
            TokioIo::new(socket),
            hyper::service::service_fn(|request| {
                std::future::ready(Ok::<_, std::convert::Infallible>(reply_to(
                    &request, path, state, reply,
                )))
            }),
        );
    let _ = timeout(CONNECTION_TIMEOUT, connection).await;
}

fn reply_to(
    request: &Request<Incoming>,
    path: &str,
    state: &str,
    reply: &Mutex<Option<Result<String, CallbackError>>>,
) -> Response<Full<Bytes>> {
    if request.method() != Method::GET || request.uri().path() != path {
        return response(StatusCode::NOT_FOUND, "Not found.");
    }
    match authorization_reply(request.uri().query().unwrap_or_default(), state) {
        Some(result) => {
            let message = if result.is_ok() {
                "Return to Axolotl Launcher to finish signing in."
            } else {
                "Microsoft sign-in was declined. Return to Axolotl Launcher."
            };
            *reply.lock().expect("callback result lock") = Some(result);
            response(StatusCode::OK, message)
        }
        None => response(
            StatusCode::BAD_REQUEST,
            "This request did not match the active sign-in.",
        ),
    }
}

fn authorization_reply(query: &str, expected: &str) -> Option<Result<String, CallbackError>> {
    let mut state = None;
    let mut code = None;
    let mut error = None;
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        let parameter = match name.as_ref() {
            "state" => &mut state,
            "code" => &mut code,
            "error" => &mut error,
            _ => continue,
        };
        if parameter.replace(value).is_some() {
            return None;
        }
    }
    if state.as_deref() != Some(expected) {
        return None;
    }
    match (code, error) {
        (Some(code), None) if !code.is_empty() => Some(Ok(code.into_owned())),
        (None, Some(error)) if !error.is_empty() => Some(Err(CallbackError::Denied)),
        _ => None,
    }
}

fn response(status: StatusCode, message: &'static str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Cache-Control", "no-store")
        .header("Referrer-Policy", "no-referrer")
        .body(Full::new(Bytes::from_static(message.as_bytes())))
        .expect("valid callback response")
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/network/src/auth/callback.rs"]
mod tests;
