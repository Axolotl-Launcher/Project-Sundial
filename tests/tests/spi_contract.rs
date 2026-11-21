// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Proves the SPI is usable the way it is meant to be: a caller written
//! against a port works with any provider, including a test double it has
//! never seen. If this stops compiling, the port has leaked a concrete type.

use std::path::{Path, PathBuf};

use superlogical_spi::{
    ContentProvider, ContentSummary, FetchError, InstanceRecord, Java, Network, StateStore,
    Storage, StorageError, Telemetry,
};
use superlogical_state::MemoryState;

struct EchoNetwork;

impl Network for EchoNetwork {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        Ok(url.as_bytes().to_vec())
    }
}

struct InMemoryStorage;

impl Storage for InMemoryStorage {
    fn read(&self, _path: &str) -> Result<Vec<u8>, StorageError> {
        Err(StorageError::NotFound("in-memory store is empty".into()))
    }

    fn write(&self, _path: &str, _bytes: &[u8]) -> Result<(), StorageError> {
        Ok(())
    }
}

struct CountingTelemetry;

impl Telemetry for CountingTelemetry {
    fn record(&self, _event: superlogical_spi::Event) {}
}

/// Written against the port, not against a provider.
async fn fetch_title(network: &impl Network, url: &str) -> Result<String, FetchError> {
    let body = network.fetch(url).await?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

#[test]
fn a_caller_written_against_the_port_works_with_any_provider() {
    let title = futures_lite_block_on(fetch_title(&EchoNetwork, "https://example.test/mod"))
        .expect("the echo provider cannot fail");
    assert_eq!(title, "https://example.test/mod");
}

#[test]
fn the_ports_are_object_safe_enough_to_store_together() {
    // Storage and Telemetry are synchronous, so they can be held as trait
    // objects; the async ports are generic instead, which is why the caller
    // above takes `&impl Network`.
    let storage: &dyn Storage = &InMemoryStorage;
    assert!(storage.read("missing").is_err());

    let telemetry: &dyn Telemetry = &CountingTelemetry;
    telemetry.record(superlogical_spi::Event {
        name: "test".into(),
    });
}

#[test]
fn the_state_provider_satisfies_the_state_port() {
    let mut store = MemoryState::default();
    let record = InstanceRecord {
        id: superlogical_spi::Id::new(1),
        name: "Test".into(),
    };
    store.save_instance(&record).unwrap();
    assert_eq!(
        store.load_instance(record.id).unwrap().as_ref(),
        Some(&record)
    );
}

#[test]
fn a_content_provider_lists_through_the_port() {
    use superlogical_content::StaticProvider;

    let provider = StaticProvider::with_items(vec![ContentSummary {
        id: superlogical_spi::Id::new(7),
        name: "Example Mod".into(),
    }]);

    let items = futures_lite_block_on(provider.list()).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "Example Mod");
}

/// Runs a future to completion on the current thread.
///
/// The async ports are plain futures with no runtime of their own, so a test
/// needs only a minimal executor. Pulling in a full runtime for four tests
/// would add a dependency this crate does not otherwise need.
fn futures_lite_block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};

    let mut future = Box::pin(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

struct FixedJava(PathBuf);

impl Java for FixedJava {
    fn executable(&self, _preferred: Option<&Path>) -> Option<PathBuf> {
        Some(self.0.clone())
    }
}

#[test]
fn a_java_provider_satisfies_the_port() {
    let java: &dyn Java = &FixedJava(PathBuf::from("/opt/jdk/bin/java"));

    assert_eq!(
        java.executable(None),
        Some(PathBuf::from("/opt/jdk/bin/java"))
    );
}
