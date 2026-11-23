// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

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
            .open_download(&url, DownloadProtocol::default(), Some(request))
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
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reader = BufReader::new(&stream);
    let mut request = String::new();
    loop {
        let previous = request.len();
        let received = reader.read_line(&mut request).unwrap();
        if received == 0 || &request[previous..] == "\r\n" {
            break;
        }
    }
    write!(stream, "{version} 206 Partial Content\r\nContent-Range: bytes 2-4/8\r\nContent-Length: 3\r\nConnection: close\r\n\r\ncde").unwrap();
    request
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
