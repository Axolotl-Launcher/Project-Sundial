//! Pipe messages and client for full-machine Java discovery.

use std::path::PathBuf;
use std::sync::OnceLock;

use axlauncher_storage::DataDir;
use serde::{Deserialize, Serialize};

use crate::{Event, Image, Service};

static WORKER: OnceLock<Service<Candidate>> = OnceLock::new();

#[derive(Deserialize, Serialize)]
pub enum Command {
    Scan,
}

#[derive(Deserialize, Serialize)]
pub struct Candidate(#[serde(with = "crate::path")] PathBuf);

impl Candidate {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn into_path(self) -> PathBuf {
        self.0
    }
}

pub(crate) fn configure(image: Image, executable: Result<PathBuf, String>) {
    let _ = WORKER.set(Service::new(image, executable));
}

pub(crate) fn close() {
    if let Some(worker) = WORKER.get() {
        worker.close();
    }
}

pub(crate) fn wait_closed() {
    if let Some(worker) = WORKER.get() {
        worker.close_and_wait();
    }
}

/// A failed or interrupted worker scan exposes none of its partial results.
pub async fn scan(data: &DataDir) -> Result<Vec<PathBuf>, String> {
    let worker = WORKER
        .get()
        .ok_or("The embedded Java discovery worker is unavailable.")?;
    let mut request = worker.request(data, Command::Scan)?;
    collect(async || request.next().await).await
}

async fn collect(
    mut next: impl AsyncFnMut() -> Option<Event<Candidate>>,
) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    while let Some(event) = next().await {
        match event {
            Event::Reply(candidate) => paths.push(candidate.into_path()),
            Event::Finished(result) => {
                result?;
                paths.sort();
                paths.dedup();
                return Ok(paths);
            }
            Event::Disconnected => {
                return Err("The Java discovery worker exited before completing the scan.".into());
            }
        }
    }
    Err("The Java discovery worker closed before completing the scan.".into())
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/src/java_discovery.rs"]
mod tests;
