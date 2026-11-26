// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use super::*;

#[test]
fn a_probe_saves_the_http1_response_under_the_data_root() {
    let root = test_tempfile::tempdir().unwrap();
    let destination = axlauncher_core::storage::DataDir::isolated(root.path())
        .unwrap()
        .download_debug_file();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || serve_http1(listener));
    let (_sender, cancellation) = oneshot::channel();

    let status = run_probe(
        url,
        destination.clone(),
        axlauncher_download_engine::Direct(
            &axlauncher_core::network::ReqwestNetwork::desktop().unwrap(),
        ),
        cancellation,
    );

    assert!(matches!(status, ProbeStatus::Complete(path) if path == destination));
    assert_eq!(std::fs::read(&destination).unwrap(), b"ok");
    server.join().unwrap();
}

fn serve_http1(listener: TcpListener) {
    let (mut socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut lines = BufReader::new(&socket).lines();
    assert_eq!(lines.next().unwrap().unwrap(), "GET /file HTTP/1.1");
    assert!(lines.map(Result::unwrap).any(|line| line.is_empty()));
    socket
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
        .unwrap();
}

#[test]
fn cancelling_a_probe_keeps_the_previous_completed_file() {
    let root = test_tempfile::tempdir().unwrap();
    let destination = root.path().join("download-debug.bin");
    std::fs::write(&destination, b"previous").unwrap();
    let (cancel, cancellation) = oneshot::channel();
    cancel.send(()).unwrap();

    let status = run_probe(
        "http://127.0.0.1:1/file".into(),
        destination.clone(),
        axlauncher_download_engine::Direct(&axlauncher_core::network::UnimplementedNetwork),
        cancellation,
    );

    assert!(matches!(status, ProbeStatus::Cancelled));
    assert_eq!(std::fs::read(destination).unwrap(), b"previous");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}
