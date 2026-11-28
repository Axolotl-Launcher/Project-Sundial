// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::thread;

use axlauncher_network_spi::auth::ApiKey;

#[test]
fn fallen_proxy_preserves_minecraft_methods_paths_and_queries() {
    let host = ServiceHost::parse("proxy.example.invalid").unwrap();
    for (path, body) in [
        ("/launcher/login", Some(Body::Json("{}".into()))),
        ("/entitlements/license?requestId=test", None),
        ("/minecraft/profile", None),
    ] {
        let original = format!("https://api.minecraftservices.com{path}");
        let mut request = Request::new(&original, "test").with_minecraft_service();
        request.body = body;
        assert_eq!(
            fallen_service_url(&request, &host).unwrap().as_str(),
            format!("https://services.proxy.example.invalid{path}")
        );
    }
}

#[test]
fn fallen_proxy_only_accepts_marked_official_minecraft_service_requests() {
    let host = ServiceHost::parse("proxy.example.invalid").unwrap();
    let unmarked = Request::new(
        "https://api.minecraftservices.com/minecraft/profile",
        "test",
    );
    assert!(fallen_service_url(&unmarked, &host).is_none());
    for url in [
        "https://login.microsoftonline.com/consumers/oauth2/v2.0/token",
        "https://xsts.auth.xboxlive.com/xsts/authorize",
        "https://third-party.example/minecraft/profile",
        "https://api.minecraftservices.com:8443/minecraft/profile",
        "https://user:password@api.minecraftservices.com/minecraft/profile",
        "http://api.minecraftservices.com/minecraft/profile",
        "https://api.minecraftservices.com/minecraft/profile/skins",
        "https://api.minecraftservices.com/launcher/login",
    ] {
        let request = Request::new(url, "test").with_minecraft_service();
        assert!(fallen_service_url(&request, &host).is_none(), "{url}");
    }
}

#[tokio::test]
async fn sends_api_key_and_user_agent_and_only_returns_safe_error_metadata() {
    let key = ApiKey::new(&"x".repeat(32)).unwrap();
    let expected = key.clone();
    let (url, server) =
        serve_once(move |stream, headers| reject_request(stream, headers, &expected));
    let response = loopback(Duration::from_secs(3))
        .send(Request::new(&url, "Axolotl-Launcher/test").with_api_key(&key))
        .await
        .unwrap();
    assert_eq!(response.status, 429);
    assert_eq!(response.retry_after_seconds, Some(12));
    assert!(response.body.is_empty());
    server.join().unwrap();
}

fn reject_request(stream: &mut TcpStream, headers: Vec<(String, String)>, key: &ApiKey) {
    let has_header = |expected: &str, text: &str| {
        headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case(expected) && value == text)
    };
    let echo = key.expose_secret();
    assert!(has_header("x-api-key", echo));
    assert!(has_header("user-agent", "Axolotl-Launcher/test"));
    write!(
        stream,
        "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 12\r\n\
        Content-Length: {}\r\nConnection: close\r\n\r\n{echo}",
        echo.len()
    )
    .unwrap();
}

#[tokio::test]
async fn a_keyed_request_does_not_follow_redirects() {
    let key = ApiKey::new(&"x".repeat(32)).unwrap();
    let (url, server) = serve_once(|stream, _| {
        stream
            .write_all(
                b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/\r\n\
            Content-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
    });
    let response = loopback(Duration::from_secs(3))
        .send(Request::new(&url, "test").with_api_key(&key))
        .await
        .unwrap();
    assert_eq!(response.status, 302);
    server.join().unwrap();
}

#[tokio::test]
async fn a_stalled_keyed_http_response_returns_timeout() {
    let (stop, wait) = std::sync::mpsc::channel();
    let (url, server) = serve_once(move |_, _| {
        let _ = wait.recv_timeout(Duration::from_secs(5));
    });
    let key = ApiKey::new(&"x".repeat(32)).unwrap();
    let result = loopback(Duration::from_millis(150))
        .send(Request::new(&url, "test").with_api_key(&key))
        .await;
    assert!(matches!(result, Err(TransportError::Timeout)));
    let _ = stop.send(());
    server.join().unwrap();
}

fn loopback(timeout: Duration) -> AuthHttp {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .unwrap();
    AuthHttp {
        client,
        fallen_proxy: None,
    }
}

fn serve_once(
    reply: impl FnOnce(&mut TcpStream, Vec<(String, String)>) + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let url = format!("http://{}/test", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let headers = request_headers(&mut stream);
        reply(&mut stream, headers);
    });
    (url, server)
}

fn request_headers(stream: &mut TcpStream) -> Vec<(String, String)> {
    BufReader::new(stream)
        .lines()
        .map(|line| line.expect("request must contain a header block"))
        .take_while(|line| !line.is_empty())
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.to_owned(), value.trim().to_owned()))
        })
        .collect()
}
