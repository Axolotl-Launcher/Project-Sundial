// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream as Client};

#[test]
fn callback_ignores_mismatched_or_duplicate_oauth_parameters() {
    assert_eq!(authorization_reply("code=abc&state=other", "state"), None);
    assert_eq!(
        authorization_reply("code=abc&state=state&state=state", "state"),
        None
    );
    assert_eq!(
        authorization_reply("code=one&code=two&state=state", "state"),
        None
    );
    assert_eq!(
        authorization_reply("code=abc&state=state&error=access_denied", "state"),
        None
    );
    assert_eq!(
        authorization_reply("error=access_denied&state=other", "state"),
        None
    );
}

#[test]
fn callback_decodes_a_matching_success_or_denial() {
    assert_eq!(
        authorization_reply("code=abc%2Bdef&state=state", "state"),
        Some(Ok("abc+def".into()))
    );
    assert_eq!(
        authorization_reply("error=access_denied&state=state", "state"),
        Some(Err(CallbackError::Denied))
    );
}

#[tokio::test]
async fn loopback_listener_waits_for_the_matching_request() {
    let callback = BrowserCallback::bind(0, "/oauth/callback").await.unwrap();
    let address = callback.listener.local_addr().unwrap();
    assert!(address.ip().is_loopback());
    let client = tokio::task::spawn_blocking(move || {
        let rejected = request(address, "/oauth/callback?code=fake&state=wrong");
        assert!(rejected.starts_with("HTTP/1.1 400"));
        let accepted = request(address, "/oauth/callback?code=valid&state=expected");
        assert!(accepted.starts_with("HTTP/1.1 200"));
        assert!(!accepted.contains("code=valid"));
    });
    let code = timeout(Duration::from_secs(5), callback.receive("expected"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(code, "valid");
    client.await.unwrap();
}

fn request(address: SocketAddr, path: &str) -> String {
    let mut connection = Client::connect_timeout(&address, Duration::from_secs(3)).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(
        connection,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    connection.read_to_string(&mut response).unwrap();
    response
}
