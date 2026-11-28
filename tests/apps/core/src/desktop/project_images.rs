// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::{
    future::{Future as _, pending},
    io::{BufRead as _, BufReader, Read as _, Write as _},
    net::TcpListener,
    sync::mpsc,
    task::{Context, Waker},
    thread,
    time::Duration,
};

use super::{project_image, run_image_request};

#[tokio::test]
async fn image_timeout_returns_while_a_blocking_lookup_is_still_running() {
    let (release, lookup) = mpsc::channel();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        run_image_request(blocked_lookup(lookup), Duration::from_millis(10)),
    )
    .await;
    release.send(()).expect("lookup can receive its result");
    assert_eq!(
        result.expect("image timeout returns independently of the lookup"),
        Err("The project image request timed out".to_owned())
    );
}

#[tokio::test]
async fn image_requests_reuse_a_connection_between_worker_calls() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || serve_two_images(listener));
    for path in ["first.png", "second.png"] {
        let url = format!("http://{address}/{path}");
        assert_eq!(project_image(url).await.unwrap(), b"img");
    }
    server.join().unwrap();
}

#[test]
fn image_request_yields_to_the_caller_and_cancels_when_dropped() {
    let (started, running) = mpsc::channel();
    let (live, cancelled) = mpsc::channel::<()>();
    let transfer = async move {
        let _live = live;
        started.send(()).unwrap();
        pending().await
    };
    let mut request = Box::pin(run_image_request(transfer, Duration::from_secs(10)));
    assert!(
        request
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    running.recv_timeout(Duration::from_secs(2)).unwrap();
    drop(request);
    assert_eq!(
        cancelled.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
}

fn serve_two_images(listener: TcpListener) {
    let (socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut connection = BufReader::new(socket);
    for _ in 0..2 {
        assert!(
            connection
                .by_ref()
                .lines()
                .any(|header| header.unwrap().is_empty())
        );
        connection
            .get_mut()
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nimg")
            .unwrap();
    }
}

async fn blocked_lookup(
    lookup: mpsc::Receiver<()>,
) -> Result<Vec<u8>, axlauncher_network_spi::FetchError> {
    let (started, running) = tokio::sync::oneshot::channel();
    let lookup = tokio::task::spawn_blocking(move || {
        started.send(()).expect("request waits for lookup to start");
        lookup.recv().expect("test releases lookup");
    });
    running.await.expect("lookup has started");
    lookup.await.expect("blocking lookup task completes");
    Ok(Vec::new())
}
