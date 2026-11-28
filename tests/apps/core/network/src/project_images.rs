// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::TcpListener;
use std::time::Duration;

use url::Url;

use super::{MAX_IMAGE_BYTES, ProjectImageError, allows_image, project_image};

#[test]
fn accepts_web_images_without_an_origin_allowlist() {
    for url in [
        "https://cdn.modrinth.com/data/project/images/comparison.png",
        "https://cdn.modrinth.com/data/cached_images/badge.webp",
        "https://media.forgecdn.net/attachments/546/809/6.gif",
        "https://github.com/user-attachments/assets/8a2dcbad-57dd-4184-ac4b-a6df311ec3be",
        "https://github-production-user-asset-6210df.s3.amazonaws.com/123/image.png?signature=value",
        "https://images.example.org/illustration.png",
        "http://images.example.org:8080/illustration.png",
    ] {
        assert!(allows_image(&Url::parse(url).expect("valid URL")), "{url}");
    }
}

#[test]
fn rejects_non_web_urls_and_embedded_credentials() {
    for url in [
        "https://user@images.example.org/illustration.png",
        "https://user:password@images.example.org/illustration.png",
        "file:///data/image.png",
        "ftp://images.example.org/illustration.png",
    ] {
        assert!(!allows_image(&Url::parse(url).expect("valid URL")), "{url}");
    }
}

#[tokio::test]
async fn reports_rejected_addresses_and_redirects_separately() {
    assert_eq!(
        project_image("file:///image.png").await,
        Err(ProjectImageError::InvalidUrl)
    );
    let response = "HTTP/1.1 302 Found\r\nLocation: http://user@127.0.0.1:1/image\r\nContent-Length: 0\r\n\r\n";
    assert!(matches!(
        fetch_response(response, 0).await,
        Err(ProjectImageError::Redirect(_))
    ));
}

#[tokio::test]
async fn preserves_http_status_and_retry_delay() {
    let busy = "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 12\r\nContent-Length: 0\r\n\r\n";
    assert_eq!(
        fetch_response(busy, 0).await,
        Err(ProjectImageError::TemporaryHttp {
            status: 429,
            retry_after: Some(Duration::from_secs(12)),
        })
    );
    let absent = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
    assert_eq!(
        fetch_response(absent, 0).await,
        Err(ProjectImageError::HttpRejected(404))
    );
}

#[tokio::test]
async fn bounds_declared_and_streamed_image_sizes() {
    let oversize = MAX_IMAGE_BYTES + 1;
    let declared = format!("HTTP/1.1 200 OK\r\nContent-Length: {oversize}\r\n\r\n");
    assert_eq!(
        fetch_response(&declared, 0).await,
        Err(ProjectImageError::TooLarge)
    );
    let streamed = "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n";
    assert_eq!(
        fetch_response(streamed, oversize).await,
        Err(ProjectImageError::TooLarge)
    );
}

async fn fetch_response(headers: &str, body_bytes: usize) -> Result<Vec<u8>, ProjectImageError> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/image", listener.local_addr().unwrap());
    let headers = headers.to_owned();
    let server = std::thread::spawn(move || serve_response(listener, &headers, body_bytes));
    let result = tokio::time::timeout(Duration::from_secs(5), project_image(&url)).await;
    let result = result.expect("local image request completes");
    server.join().unwrap();
    result
}

fn serve_response(listener: TcpListener, headers: &str, mut body_bytes: usize) {
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert!(
        BufReader::new(&stream)
            .lines()
            .any(|line| line.unwrap().is_empty())
    );
    stream.write_all(headers.as_bytes()).unwrap();
    let chunk = [0; 16 * 1024];
    while body_bytes > 0 {
        let length = body_bytes.min(chunk.len());
        if stream.write_all(&chunk[..length]).is_err() {
            break;
        }
        body_bytes -= length;
    }
}
