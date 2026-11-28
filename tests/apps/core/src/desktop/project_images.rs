// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::{sync::mpsc, thread, time::Duration};

use super::run_image_request;

#[test]
fn image_timeout_returns_while_a_blocking_lookup_is_still_running() {
    let (release, lookup) = mpsc::channel();
    let (complete, response) = mpsc::channel();
    let worker = thread::spawn(move || {
        let result = run_image_request(blocked_lookup(lookup), Duration::from_millis(10));
        complete.send(result).expect("test collects response");
    });

    let result = response.recv_timeout(Duration::from_secs(2));
    release.send(()).expect("lookup can receive its result");
    worker.join().expect("image worker exits");
    assert_eq!(
        result.expect("image timeout returns independently of the lookup"),
        Err("The project image request timed out".to_owned())
    );
}

async fn blocked_lookup(
    lookup: mpsc::Receiver<()>,
) -> Result<Vec<u8>, axlauncher_network_spi::FetchError> {
    let (started, running) = mpsc::channel();
    let lookup = tokio::task::spawn_blocking(move || {
        started.send(()).expect("request waits for lookup to start");
        lookup.recv().expect("test releases lookup");
    });
    running.recv().expect("lookup has started");
    lookup.await.expect("blocking lookup task completes");
    Ok(Vec::new())
}
