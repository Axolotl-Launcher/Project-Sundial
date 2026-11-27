// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Proves the SPI is usable the way it is meant to be: a caller written
//! against a port works with any provider, including a test double it has
//! never seen. If this stops compiling, the port has leaked a concrete type.

use std::path::{Path, PathBuf};

use axlauncher_network_spi::{
    ByteRange, DownloadBody, DownloadProtocol, FetchError, Network, RangeRequest,
};
use axlauncher_spi::{
    ContentProvider, ContentSummary, InstanceRecord, Java, JavaMode, StateStore, Storage,
    StorageError, Telemetry,
};
use axlauncher_state::MemoryState;

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
    fn record(&self, _event: axlauncher_spi::Event) {}
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
fn a_body_only_provider_returns_the_full_body_for_a_conditional_range() {
    futures_lite_block_on(async {
        let url = "https://example.test/file";
        let range = RangeRequest {
            interval: ByteRange { start: 1, end: 2 },
            if_range: Some(b"\"version\""),
        };
        let mut response = EchoNetwork
            .open_download(url, DownloadProtocol::Http1, Some(range))
            .await
            .unwrap();
        assert!(response.range.is_none());
        assert!(response.representation.is_none());
        let content = response.body.next_chunk().await.unwrap().unwrap();
        assert_eq!(content, url.as_bytes());
    });
}

#[test]
fn the_ports_are_object_safe_enough_to_store_together() {
    // Storage and Telemetry are synchronous, so they can be held as trait
    // objects; the async ports are generic instead, which is why the caller
    // above takes `&impl Network`.
    let storage: &dyn Storage = &InMemoryStorage;
    assert!(storage.read("missing").is_err());

    let telemetry: &dyn Telemetry = &CountingTelemetry;
    telemetry.record(axlauncher_spi::Event {
        name: "test".into(),
    });
}

#[test]
fn the_state_provider_satisfies_the_state_port() {
    let mut store = MemoryState::default();
    let record = InstanceRecord {
        id: axlauncher_spi::Id::new(1),
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
    use axlauncher_content::StaticProvider;

    let provider = StaticProvider::with_items(vec![ContentSummary {
        id: axlauncher_spi::Id::new(7),
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
    fn executable(&self, _mode: JavaMode) -> Option<PathBuf> {
        Some(self.0.clone())
    }

    fn executable_for(&self, _major: u32, _mode: JavaMode) -> Option<PathBuf> {
        Some(self.0.clone())
    }

    fn executable_at(&self, path: &Path, _major: Option<u32>, _mode: JavaMode) -> Option<PathBuf> {
        (self.0 == path).then(|| self.0.clone())
    }
}

#[test]
fn a_java_provider_satisfies_the_port() {
    let java: &dyn Java = &FixedJava(PathBuf::from("/opt/jdk/bin/java"));

    assert_eq!(
        java.executable(JavaMode::Windowed),
        Some(PathBuf::from("/opt/jdk/bin/java"))
    );
}

/// A loader provider the contract test has never seen, standing in for any
/// other.
struct FixedLoader;

impl axlauncher_spi::ModLoader for FixedLoader {
    fn kind(&self) -> axlauncher_spi::LoaderKind {
        axlauncher_spi::LoaderKind::Fabric
    }

    async fn versions(
        &self,
        _game_version: &str,
    ) -> Result<Vec<axlauncher_spi::LoaderVersion>, axlauncher_spi::LoaderError> {
        Ok(vec![axlauncher_spi::LoaderVersion {
            version: "0.20.1".into(),
            stable: Some(true),
        }])
    }

    async fn profile(
        &self,
        request: axlauncher_spi::LoaderRequest,
    ) -> Result<axlauncher_spi::LoaderProfile, axlauncher_spi::LoaderError> {
        Ok(axlauncher_spi::LoaderProfile {
            version_id: format!("{}-0.20.1", request.game_version),
            document: serde_json::to_vec(&serde_json::json!({
                "id": "fabric-loader-0.20.1-1.21.11",
                "inheritsFrom": request.game_version,
            }))
            .expect("profile document"),
            files: vec![axlauncher_spi::LoaderFile {
                path: "libraries/example.jar".into(),
                bytes: b"jar".to_vec(),
            }],
        })
    }
}

/// Written against the port: it installs whatever the provider resolves,
/// without naming a loader.
fn install_from(
    loader: &impl axlauncher_spi::ModLoader,
    game_root: &Path,
    game_version: &str,
) -> Result<Vec<String>, String> {
    let profile = futures_lite_block_on(loader.profile(axlauncher_spi::LoaderRequest {
        game_version: game_version.into(),
        loader_version: None,
    }))
    .map_err(|error| error.to_string())?;

    let placed = axlauncher_install::loader::place(
        &axlauncher_storage::GameDir::standalone(game_root),
        &profile,
    )
    .map_err(|error| error.to_string())?;
    Ok(vec![
        placed.version_id,
        placed.document.display().to_string(),
    ])
}

#[test]
fn a_caller_written_against_the_loader_port_works_with_any_provider() {
    let root = std::env::temp_dir().join(format!("axlauncher-spi-contract-{}", std::process::id()));
    let version = root.join("versions/1.21.11");
    std::fs::create_dir_all(&version).expect("parent version directory");
    std::fs::write(version.join("1.21.11.json"), br#"{"id":"1.21.11"}"#)
        .expect("parent version document");

    let installed = install_from(&FixedLoader, &root, "1.21.11").expect("install");

    assert_eq!(installed[0], "1.21.11-0.20.1");
    assert!(
        Path::new(&installed[1]).is_file(),
        "the version document is where a launch looks for it"
    );
    assert!(root.join("libraries/example.jar").is_file());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_loader_kind_is_read_back_from_its_stored_spelling() {
    assert_eq!(
        axlauncher_spi::LoaderKind::parse("quilt"),
        Some(axlauncher_spi::LoaderKind::Quilt)
    );
}
