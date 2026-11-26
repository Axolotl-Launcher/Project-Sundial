// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};

use axlauncher_spi::{DownloadProtocol, Network};

use crate::ReqwestNetwork;

#[path = "../../../../support/http2.rs"]
mod http2;

#[test]
fn reqwest_reports_a_partial_response_without_sending_a_validator() {
    for response_version in ["HTTP/1.1", "HTTP/1.0"] {
        assert_partial_response(response_version);
    }
}

fn assert_partial_response(response_version: &'static str) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let worker = std::thread::spawn(move || serve_range(listener, response_version));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async {
        let network = ReqwestNetwork::desktop().unwrap();
        let request = ByteRange { start: 2, end: 5 };
        let mut response = network
            .open_download(
                &url,
                DownloadProtocol::default(),
                Some(RangeRequest {
                    interval: request,
                    if_range: None,
                }),
            )
            .await
            .unwrap();
        assert_eq!(response.length, Some(3));
        assert_eq!(
            response.range,
            Some(ResponseRange {
                interval: request,
                total: 8
            })
        );
        assert_eq!(
            response.body.next_chunk().await.unwrap().unwrap(),
            b"cde"[..]
        );
        assert!(response.body.next_chunk().await.unwrap().is_none());
    });

    let request = worker.join().unwrap().to_ascii_lowercase();
    assert!(request.starts_with("get /file http/1.1\r\n"));
    assert!(request.contains("\r\nrange: bytes=2-4\r\n"));
    assert!(!request.contains("\r\nif-range:"));
    assert!(request.contains("\r\naccept-encoding: identity\r\n"));
}

fn serve_range(listener: TcpListener, version: &str) -> String {
    let (mut stream, _) = listener.accept().unwrap();
    let request = read_request(&stream);
    write!(stream, "{version} 206 Partial Content\r\nContent-Range: bytes 2-4/8\r\nContent-Length: 3\r\nConnection: close\r\n\r\ncde").unwrap();
    request
}

fn read_request(stream: &TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    loop {
        let previous = request.len();
        let received = reader.read_line(&mut request).unwrap();
        if received == 0 || &request[previous..] == "\r\n" {
            break;
        }
    }
    request
}

#[test]
fn retry_after_accepts_seconds_and_dates_at_response_receipt() {
    let now = httpdate::parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT").unwrap();
    assert_eq!(retry_delay(" 42 ", now), Some(Duration::from_secs(42)));
    assert_eq!(
        retry_delay("Sun, 06 Nov 1994 08:49:40 GMT", now),
        Some(Duration::from_secs(3))
    );
    assert_eq!(
        retry_delay("Sun, 06 Nov 1994 08:49:30 GMT", now),
        Some(Duration::ZERO)
    );
    assert_eq!(
        retry_delay("9999999999999999999999999999", now),
        Some(Duration::from_secs(u64::MAX))
    );
    for invalid in ["", "-5", "+5", "1.2", "tomorrow"] {
        assert_eq!(retry_delay(invalid, now), None);
    }
}

#[test]
fn http_errors_are_classified_before_reading_the_body() {
    for (status, header) in [(429, "8"), (503, "invalid"), (501, "8"), (404, "8")] {
        assert_download_status(status, header);
    }
}

fn assert_download_status(status: u16, header: &'static str) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let (release, hold) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_request(&stream);
        write!(
            stream,
            "HTTP/1.1 {status} Error\r\nRetry-After: {header}\r\nContent-Length: 20\r\n\r\n"
        )
        .unwrap();
        hold.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = runtime.block_on(async {
        let network = ReqwestNetwork::desktop().unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            network.open_download(&url, DownloadProtocol::Http1, None),
        )
        .await
        .unwrap()
        .err()
        .unwrap()
    });
    release.send(()).unwrap();
    worker.join().unwrap();
    let expected = match status {
        429 => FetchError::HttpRetryable {
            status,
            retry_after: Some(Duration::from_secs(8)),
        },
        503 => FetchError::HttpRetryable {
            status,
            retry_after: None,
        },
        _ => FetchError::Rejected {
            status,
            body: Vec::new(),
        },
    };
    assert_eq!(result, expected);
}

#[test]
fn strong_tags_and_effective_range_resources_are_preserved() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let server = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let (mut initial, _) = listener.accept().unwrap();
        assert!(read_request(&initial).starts_with("GET /first "));
        write!(initial, "HTTP/1.1 302 Found\r\nLocation: /file?q=1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        let (mut redirected, _) = listener.accept().unwrap();
        let headers = read_request(&redirected).to_ascii_lowercase();
        assert!(headers.starts_with("get /file?q=1 "));
        assert!(headers.contains("\r\nif-range: \"first\"\r\n"));
        write!(redirected, "HTTP/1.1 206 Partial Content\r\nETag: \"second\"\r\nContent-Range: bytes 0-0/8\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx").unwrap();
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let network = ReqwestNetwork::desktop().unwrap();
        let url = format!("http://{server}/first");
        let range = RangeRequest {
            interval: ByteRange { start: 0, end: 1 },
            if_range: Some(b"\"first\""),
        };
        let response = network
            .open_download(&url, DownloadProtocol::Http1, Some(range))
            .await
            .unwrap();
        let identity = response.representation.unwrap();
        assert_eq!(identity.resource, format!("http://{server}/file?q=1"));
        assert_eq!(
            identity.etag,
            Some(DownloadEtag::Strong(Bytes::from_static(b"\"second\"")))
        );
    });
    worker.join().unwrap();
    assert_eq!(
        entity_tag(&HeaderValue::from_static("W/\"weak\"")),
        DownloadEtag::Unusable
    );
}

#[test]
fn the_desktop_file_client_transfers_over_http2_when_requested() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let worker = std::thread::spawn(move || http2::serve_http2(listener, *b"h2"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async {
        let network = ReqwestNetwork::desktop().unwrap();
        let mut response = network
            .open_download(&url, DownloadProtocol::Http2, None)
            .await
            .unwrap();
        assert_eq!(
            response.body.next_chunk().await.unwrap().unwrap(),
            b"h2"[..]
        );
        assert!(response.body.next_chunk().await.unwrap().is_none());
    });

    worker.join().unwrap();
}
