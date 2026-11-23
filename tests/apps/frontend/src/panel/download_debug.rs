// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::net::TcpListener;

use super::*;

#[path = "../../../../support/http2.rs"]
mod http2;

#[test]
fn a_probe_saves_the_http2_response_under_the_data_root() {
    let root = test_tempfile::tempdir().unwrap();
    let destination = DataDir::new(root.path()).download_debug_file();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || http2::serve_http2(listener, *b"ok"));
    let (_sender, cancellation) = oneshot::channel();

    let status = run_probe(url, destination.clone(), cancellation);

    assert!(matches!(status, ProbeStatus::Complete(path) if path == destination));
    assert_eq!(std::fs::read(&destination).unwrap(), b"ok");
    server.join().unwrap();
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
        cancellation,
    );

    assert!(matches!(status, ProbeStatus::Cancelled));
    assert_eq!(std::fs::read(destination).unwrap(), b"previous");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}
