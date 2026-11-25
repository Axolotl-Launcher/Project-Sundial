// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

use super::*;
use crate::panel::download_debug::test_tempfile;

#[test]
fn every_row_of_a_fifty_link_batch_is_reachable() {
    let total = 50usize;
    let pages = total.div_ceil(RESULTS_PER_PAGE);
    let visible: Vec<_> = (1..=pages)
        .flat_map(|page| result_page_range(total, page))
        .collect();
    assert_eq!(result_page_range(total, 1), 0..10);
    assert_eq!(result_page_range(total, pages), 40..50);
    assert!(visible.into_iter().eq(0..total));
}

#[test]
fn a_partial_final_page_has_no_empty_rows() {
    assert_eq!(result_page_range(53, 6), 50..53);
    assert_eq!(result_page_range(53, 99), 50..53);
    assert_eq!(result_page_range(0, 1), 0..0);
}

#[test]
fn pasted_links_keep_original_line_numbers_and_duplicate_urls() {
    let links =
        parse_links("  https://example.invalid/file \r\n\r\nhttps://example.invalid/file\n ");
    let lines: Vec<_> = links
        .iter()
        .map(|link| (link.line, link.url.as_str()))
        .collect();
    assert_eq!(
        lines,
        [
            (1, "https://example.invalid/file"),
            (3, "https://example.invalid/file")
        ]
    );
}

#[test]
fn a_failed_link_does_not_prevent_other_outputs() {
    let root = test_tempfile::tempdir().unwrap();
    let data = DataDir::new(root.path());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || serve_http1(listener));
    let links = parse_links(&format!("http://{address}/ok\n\ninvalid-url"));
    let (_cancel, cancellation) = oneshot::channel();
    let (report, mut outcomes) = mpsc::unbounded_channel();

    let options = InstallOptions {
        concurrency: 2,
        ..InstallOptions::from_settings(None)
    };
    let batch = run_batch(&data, links, options, cancellation, report).unwrap();

    assert!(!batch.cancelled);
    let mut finished = [outcomes.try_recv().unwrap(), outcomes.try_recv().unwrap()];
    finished.sort_by_key(|outcome| output_line(&outcome.path));
    assert!(finished[0].result.is_ok());
    assert!(finished[1].result.is_err());
    assert_eq!(std::fs::read(batch.directory.join("1.bin")).unwrap(), b"ok");
    assert!(!batch.directory.join("3.bin").exists());
    let another_batch = data.new_download_debug_batch().unwrap();
    assert_ne!(batch.directory, another_batch);
    assert!(batch.directory.join("1.bin").is_file());
    server.join().unwrap();
}

#[test]
fn cancelling_a_batch_preserves_the_files_already_saved() {
    let root = test_tempfile::tempdir().unwrap();
    let data = DataDir::new(root.path());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let stall = TcpListener::bind("127.0.0.1:0").unwrap();
    let links = parse_links(&format!(
        "http://{}/ok\nhttp://{}/stall",
        listener.local_addr().unwrap(),
        stall.local_addr().unwrap()
    ));
    let server = std::thread::spawn(move || serve_http1(listener));
    let (cancel, cancellation) = oneshot::channel();
    let (report, mut outcomes) = mpsc::unbounded_channel();
    let options = InstallOptions {
        concurrency: 1,
        ..InstallOptions::from_settings(None)
    };
    let worker =
        std::thread::spawn(move || run_batch(&data, links, options, cancellation, report).unwrap());

    let completed = outcomes.blocking_recv().expect("successful first transfer");
    cancel.send(()).unwrap();
    let batch = worker.join().unwrap();

    assert!(batch.cancelled);
    assert!(completed.result.is_ok());
    assert_eq!(std::fs::read(batch.directory.join("1.bin")).unwrap(), b"ok");
    assert_eq!(std::fs::read_dir(&batch.directory).unwrap().count(), 1);
    server.join().unwrap();
}

fn serve_http1(listener: TcpListener) {
    let (mut socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut lines = BufReader::new(&socket).lines();
    assert_eq!(lines.next().unwrap().unwrap(), "GET /ok HTTP/1.1");
    assert!(lines.map(Result::unwrap).any(|line| line.is_empty()));
    socket
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
        .unwrap();
}
