// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use axlauncher_network_spi::{FetchError, Network};
use axlauncher_spi::LoaderFile;

fn profile(id: &str) -> LoaderProfile {
    LoaderProfile {
        version_id: id.into(),
        document: format!(r#"{{"id":"{id}"}}"#).into_bytes(),
        files: Vec::new(),
    }
}

fn temp() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("axlauncher-place-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&path).expect("temp dir");
    path
}

#[test]
fn a_profile_lands_where_a_launch_looks_for_it() {
    let root = temp();
    let game = GameDir::standalone(&root);
    let parent = game.version_path("1.21.11", VersionPath::Document);
    write(
        &parent,
        br#"{"id":"1.21.11","downloads":{"client":{"url":"https://example.com/client.jar"}}}"#,
    )
    .expect("parent");
    let mut profile = profile("1.21.11-0.20.1");
    profile.document = br#"{"id":"fabric-loader-0.20.1-1.21.11","inheritsFrom":"1.21.11","mainClass":"net.example.Fabric"}"#.to_vec();
    let placed = place(&game, &profile).expect("place");

    assert_eq!(
        placed.document,
        root.join("versions/1.21.11-0.20.1/1.21.11-0.20.1.json")
    );
    let installed =
        axlauncher_launch::version::load(&game, &placed.version_id).expect("read merged profile");
    assert_eq!(installed.main_class, "net.example.Fabric");
    assert_eq!(installed.inherits_from, None);
    assert_eq!(
        installed.jar_id(),
        "1.21.11",
        "the parent's client jar runs"
    );
    let client = crate::assets::client_jar(&game, &installed).expect("client download");
    assert_eq!(client.path, root.join("versions/1.21.11/1.21.11.jar"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_files_a_loader_brings_are_written_under_the_root() {
    let root = temp();
    let mut profile = profile("quilt-loader-1-1.21");
    profile.files = vec![LoaderFile {
        path: "libraries/net/example/1/example-1.jar".into(),
        bytes: b"jar".to_vec(),
    }];

    let placed = place(&GameDir::standalone(&root), &profile).expect("place");

    assert_eq!(placed.files, 1);
    assert_eq!(
        std::fs::read(root.join("libraries/net/example/1/example-1.jar")).expect("file"),
        b"jar"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_file_path_that_leaves_the_root_is_refused() {
    let root = temp();
    for path in [
        "../escape.jar",
        "libraries/../../escape.jar",
        "/absolute.jar",
        "libraries\\windows.jar",
        "C:/absolute.jar",
        "libraries//double.jar",
        "./libraries/file.jar",
    ] {
        let mut profile = profile("loader-1");
        profile.files = vec![LoaderFile {
            path: path.into(),
            bytes: Vec::new(),
        }];

        let error = place(&GameDir::standalone(&root), &profile).expect_err(path);
        assert!(matches!(error, PlaceError::InvalidPath { .. }), "{path}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_id_that_would_replace_the_vanilla_document_is_refused() {
    let root = temp();
    let mut inherited = profile("1.20.1");
    inherited.document = br#"{"id":"1.20.1","inheritsFrom":"1.20.1"}"#.to_vec();

    let error = place(&GameDir::standalone(&root), &inherited).expect_err("would replace vanilla");

    assert!(matches!(error, PlaceError::InvalidId { .. }), "{error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_id_that_cannot_be_a_directory_is_refused() {
    let root = temp();
    for id in [
        "",
        ".",
        "..",
        "a/b",
        "a\\b",
        "../x",
        &"x".repeat(MAX_ID_LEN + 1),
    ] {
        let error = place(&GameDir::standalone(&root), &profile(id)).expect_err(id);
        assert!(matches!(error, PlaceError::InvalidId { .. }), "{id:?}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_loader_version_with_dots_and_pluses_is_a_usable_id() {
    for id in [
        "fabric-loader-0.20.1-1.21.11",
        "quilt-loader-0.20.0-beta.9-1.21.11",
        "1.20.1-forge-47.2.0",
    ] {
        assert!(check_id(id).is_ok(), "{id}");
    }
}

const CLIENT_JAR: &[u8] = b"client jar";
const CLIENT_SHA1: &str = "e0ede96191ad4f38d167edb4cad56c99fda06502";
const CLIENT_URL: &str = "https://example.test/client.jar";

/// A Java port with one answer for every request.
struct FixedJava(Option<PathBuf>);

impl axlauncher_spi::Java for FixedJava {
    fn executable(&self, _mode: axlauncher_spi::JavaMode) -> Option<PathBuf> {
        self.0.clone()
    }

    fn executable_for(&self, _major: u32, _mode: axlauncher_spi::JavaMode) -> Option<PathBuf> {
        self.0.clone()
    }

    fn executable_at(
        &self,
        path: &Path,
        _major: Option<u32>,
        _mode: axlauncher_spi::JavaMode,
    ) -> Option<PathBuf> {
        self.0.clone().filter(|executable| executable == path)
    }
}

struct FakeNetwork {
    responses: std::collections::BTreeMap<String, Vec<u8>>,
}

impl Network for FakeNetwork {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.responses
            .get(url)
            .cloned()
            .ok_or_else(|| FetchError::Fatal(format!("unexpected URL: {url}")))
    }
}

/// A provider the install never heard of, standing in for any loader.
struct FakeLoader {
    fail: bool,
    files: Vec<LoaderFile>,
    processors: bool,
}

impl axlauncher_spi::ModLoader for FakeLoader {
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
    ) -> Result<LoaderProfile, axlauncher_spi::LoaderError> {
        if self.fail {
            return Err(axlauncher_spi::LoaderError::NoVersions {
                loader: self.kind(),
                game_version: request.game_version,
            });
        }
        let processors = if self.processors {
            r#","processors":[{"jar":"net.example:tool:1","classpath":[],"args":[]}]"#
        } else {
            ""
        };
        Ok(LoaderProfile {
            version_id: format!("fabric-loader-0.20.1-{}", request.game_version),
            document: format!(
                r#"{{"id":"fabric-loader-0.20.1-{}","inheritsFrom":"{}","mainClass":"net.example.Fabric"{processors}}}"#,
                request.game_version, request.game_version
            )
            .into_bytes(),
            files: self.files.clone(),
        })
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};

    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = std::pin::pin!(future);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

fn vanilla_root() -> PathBuf {
    let root = temp();
    let document = format!(
        r#"{{"id":"1.21.11","type":"release","mainClass":"net.minecraft.client.main.Main","assets":"34","downloads":{{"client":{{"url":"{CLIENT_URL}","sha1":"{CLIENT_SHA1}","size":{}}}}}}}"#,
        CLIENT_JAR.len()
    );
    write(
        &GameDir::standalone(&root).version_path("1.21.11", VersionPath::Document),
        document.as_bytes(),
    )
    .expect("vanilla document");
    root
}

fn install_once(
    network: &FakeNetwork,
    loader: &FakeLoader,
    java: &FixedJava,
    root: &Path,
    report: impl FnMut(crate::prepare::Progress),
) -> Result<LoaderInstalled, LoaderInstallError> {
    let target = axlauncher_launch::Target::host();
    block_on(
        InstallRequest {
            network,
            downloader: &axlauncher_download_engine::Direct(network),
            loader,
            java,
            game_root: &GameDir::standalone(root),
            request: request(),
            target: &target,
            concurrency: 1,
        }
        .install(report),
    )
}

fn request() -> axlauncher_spi::LoaderRequest {
    axlauncher_spi::LoaderRequest {
        game_version: "1.21.11".into(),
        loader_version: Some("0.20.1".into()),
    }
}

#[test]
fn installing_a_loader_writes_the_profile_and_its_files() {
    let root = vanilla_root();
    let network = FakeNetwork {
        responses: std::collections::BTreeMap::from([(CLIENT_URL.into(), CLIENT_JAR.to_vec())]),
    };
    let loader = FakeLoader {
        fail: false,
        processors: false,
        files: vec![LoaderFile {
            path: "libraries/example/loader.jar".into(),
            bytes: b"jar".to_vec(),
        }],
    };

    let installed =
        install_once(&network, &loader, &FixedJava(None), &root, |_| {}).expect("install");

    assert_eq!(installed.version_id, "fabric-loader-0.20.1-1.21.11");
    assert!(installed.prepared.is_complete(), "{:?}", installed.prepared);
    let game = GameDir::standalone(&root);
    assert!(
        game.version_path(&installed.version_id, VersionPath::Document)
            .is_file()
    );
    assert!(root.join("libraries/example/loader.jar").is_file());
    let target = axlauncher_launch::Target::host();
    check_files(&game, &installed.version_id, &target).expect("installed loader files");
    std::fs::remove_file(root.join("versions/1.21.11/1.21.11.jar")).expect("remove inherited jar");
    let error = check_files(&game, &installed.version_id, &target).expect_err("missing client");
    assert!(error.contains("1.21.11.jar"), "{error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_loader_the_provider_cannot_resolve_is_reported() {
    let root = vanilla_root();
    let network = FakeNetwork {
        responses: std::collections::BTreeMap::new(),
    };
    let loader = FakeLoader {
        fail: true,
        processors: false,
        files: Vec::new(),
    };

    let error = install_once(&network, &loader, &FixedJava(None), &root, |_| {})
        .expect_err("provider error");

    assert!(matches!(error, LoaderInstallError::Provider(_)), "{error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_progress_reports_the_loader_phase() {
    let root = vanilla_root();
    let network = FakeNetwork {
        responses: std::collections::BTreeMap::from([(CLIENT_URL.into(), CLIENT_JAR.to_vec())]),
    };
    let loader = FakeLoader {
        fail: false,
        processors: false,
        files: Vec::new(),
    };
    let mut seen = Vec::new();

    install_once(&network, &loader, &FixedJava(None), &root, |progress| {
        seen.push(progress)
    })
    .expect("install");

    assert!(seen.contains(&crate::prepare::Progress::Loader), "{seen:?}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_profile_with_processors_needs_the_java_its_document_asks_for() {
    let root = vanilla_root();
    let network = FakeNetwork {
        responses: std::collections::BTreeMap::from([(CLIENT_URL.into(), CLIENT_JAR.to_vec())]),
    };
    let loader = FakeLoader {
        fail: false,
        processors: true,
        files: Vec::new(),
    };

    let error =
        install_once(&network, &loader, &FixedJava(None), &root, |_| {}).expect_err("no java");

    assert!(
        matches!(error, LoaderInstallError::Java { required: 8 }),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
