// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;

use super::*;
use axlauncher_spi::{FetchError, Network};

const RAW_BODY: &[u8] = b"java binary";
const PLAIN_BODY: &[u8] = b"plain file";

fn sha1(bytes: &[u8]) -> String {
    axlauncher_network::verify(bytes, &hex_sha1(bytes));
    hex_sha1(bytes)
}

fn hex_sha1(bytes: &[u8]) -> String {
    let mut hash = crate::test_sha1::Sha1::new();
    hash.update(bytes);
    hash.digest().to_string()
}

fn lzma(bytes: &[u8]) -> Vec<u8> {
    let options = xz2::stream::LzmaOptions::new_preset(6).expect("options");
    let stream = xz2::stream::Stream::new_lzma_encoder(&options).expect("encoder");
    let mut encoder = xz2::write::XzEncoder::new_stream(Vec::new(), stream);
    encoder.write_all(bytes).expect("compress");
    encoder.finish().expect("finish")
}

struct FakeNetwork {
    responses: BTreeMap<String, Vec<u8>>,
}

impl Network for FakeNetwork {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.responses
            .get(url)
            .cloned()
            .ok_or_else(|| FetchError::Fatal(format!("unexpected URL: {url}")))
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

fn temp() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "axlauncher-runtime-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("temp");
    path
}

/// The index, manifest, and objects a minimal runtime is made of.
///
/// The manifest's names are relative to the component directory, as Mojang's
/// are: a macOS runtime is a bundle, so its names carry the bundle's home.
fn fixture(platform: &str) -> (Vec<u8>, BTreeMap<String, Vec<u8>>) {
    let manifest_url = "https://example.test/manifest.json";
    let raw_url = "https://example.test/java";
    let plain_url = "https://example.test/plain";
    let packed_url = "https://example.test/packed";
    let home = if platform.starts_with("mac-os") {
        "jre.bundle/Contents/Home/"
    } else {
        ""
    };

    let packed = lzma(PLAIN_BODY);
    let manifest = format!(
        r#"{{"files": {{
          "{home}bin/{java_bin}": {{"type":"file","executable":true,"downloads":{{"raw":{{"url":"{raw_url}","sha1":"{raw_sha}"}}}}}},
          "{home}lib/plain.txt": {{"type":"file","executable":false,"downloads":{{"raw":{{"url":"{plain_url}","sha1":"{plain_sha}"}}}}}},
          "{home}lib/packed.txt": {{"type":"file","executable":false,"downloads":{{
            "lzma":{{"url":"{packed_url}","sha1":"{packed_sha}"}},
            "raw":{{"url":"{plain_url}","sha1":"{plain_sha}"}}
          }}}},
          "{home}lib": {{"type":"directory"}}
        }}}}"#,
        java_bin = JAVA_BIN,
        raw_sha = sha1(RAW_BODY),
        plain_sha = sha1(PLAIN_BODY),
        packed_sha = sha1(&packed),
    );
    let index = format!(
        r#"{{"{platform}": {{"java-runtime-delta": [{{
          "version": {{"name": "21.0.7"}},
          "manifest": {{"url": "{manifest_url}", "sha1": "{manifest_sha}"}}
        }}]}}}}"#,
        manifest_sha = sha1(manifest.as_bytes()),
    );

    let mut responses = BTreeMap::new();
    responses.insert(RUNTIME_INDEX.to_string(), index.into_bytes());
    responses.insert(manifest_url.to_string(), manifest.into_bytes());
    responses.insert(raw_url.to_string(), RAW_BODY.to_vec());
    responses.insert(plain_url.to_string(), PLAIN_BODY.to_vec());
    responses.insert(packed_url.to_string(), packed);
    (Vec::new(), responses)
}

/// The fixture writes manifest names, and the installer resolves a runtime's
/// home from the component directory. On macOS those two disagree unless the
/// names carry the bundle, which only a macOS run would otherwise reveal.
#[test]
fn the_fixture_names_its_files_where_the_platform_puts_the_home() {
    for platform in ["windows-x64", "linux", "mac-os-arm64"] {
        let (_, responses) = fixture(platform);
        let manifest: serde_json::Value =
            serde_json::from_slice(&responses["https://example.test/manifest.json"])
                .expect("manifest");
        let java = home_of(Path::new("component"), platform)
            .join("bin")
            .join(JAVA_BIN);
        let names: Vec<&String> = manifest["files"]
            .as_object()
            .expect("files")
            .keys()
            .collect();

        assert!(
            names
                .iter()
                .any(|name| safe_join(Path::new("component"), name).as_deref()
                    == Some(java.as_path())),
            "{platform} puts java at {}, but the fixture names {names:?}",
            java.display()
        );
    }
}

#[test]
fn a_runtime_lands_with_verified_files_and_an_executable_java() {
    let Some(platform) = platform() else {
        eprintln!("no published runtime for this platform; skipping");
        return;
    };
    let root = temp();
    let (_, responses) = fixture(platform);
    let network = FakeNetwork { responses };

    let installed = block_on(install(&network, &root, 21, |_| {})).expect("install");

    assert_eq!(installed.full_version, "21.0.7");
    assert_eq!(installed.component, "java-runtime-delta");
    assert!(installed.java.is_file(), "{}", installed.java.display());
    assert_eq!(fs::read(&installed.java).expect("java"), RAW_BODY);
    let home = home_of(&root.join("java-runtime-delta"), platform);
    assert_eq!(
        fs::read(home.join("lib/packed.txt")).expect("packed"),
        PLAIN_BODY,
        "an lzma object is decompressed and checked against its raw hash"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&installed.java)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "java must be executable");
    }
    let _ = fs::remove_dir_all(&root);
}

/// Installs once, so later assertions start from a tree this launcher made.
fn install_once(platform: &str, root: &Path) -> (PathBuf, BTreeMap<String, Vec<u8>>) {
    let (_, responses) = fixture(platform);
    let installed = block_on(install(
        &FakeNetwork {
            responses: responses.clone(),
        },
        root,
        21,
        |_| {},
    ))
    .expect("install");
    let _ = installed;
    (root.join("java-runtime-delta"), responses)
}

/// The index alone: a second manifest or object fetch fails the call.
fn index_only(responses: &BTreeMap<String, Vec<u8>>) -> FakeNetwork {
    FakeNetwork {
        responses: BTreeMap::from([(
            RUNTIME_INDEX.to_string(),
            responses.get(RUNTIME_INDEX).cloned().expect("index"),
        )]),
    }
}

/// A killed install leaves no record, so a tree that looks finished is rebuilt
/// rather than trusted.
#[test]
fn a_runtime_without_the_record_is_downloaded_again() {
    let Some(platform) = platform() else {
        return;
    };
    let root = temp();
    let (_, responses) = fixture(platform);
    let directory = root.join("java-runtime-delta");
    let home = home_of(&directory, platform);
    let java = home.join("bin").join(JAVA_BIN);
    fs::create_dir_all(java.parent().expect("parent")).expect("dir");
    fs::write(&java, b"truncated").expect("seed");
    fs::write(home.join("release"), b"JAVA_VERSION=\"21.0.7\"\n").expect("release");

    let installed =
        block_on(install(&FakeNetwork { responses }, &root, 21, |_| {})).expect("install");

    assert_eq!(fs::read(&installed.java).expect("java"), RAW_BODY);
    assert!(
        record::marker_path(&directory).is_file(),
        "the record is written"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_complete_runtime_is_reused_without_a_network_round_trip() {
    let Some(platform) = platform() else {
        return;
    };
    let root = temp();
    let (directory, responses) = install_once(platform, &root);

    // Every fetch but the index fails, so a reuse that needs the manifest would
    // error instead of returning.
    let installed = block_on(install(&index_only(&responses), &root, 21, |_| {})).expect("reuse");

    assert_eq!(
        installed.java,
        directory
            .join(home_of(Path::new(""), platform))
            .join("bin")
            .join(JAVA_BIN)
    );
    let _ = fs::remove_dir_all(&root);
}

/// A file that was there when the record was written and is gone now makes the
/// runtime unusable, so it is rebuilt rather than reported as installed.
#[test]
fn a_runtime_that_lost_a_file_is_downloaded_again() {
    let Some(platform) = platform() else {
        return;
    };
    let root = temp();
    let (directory, responses) = install_once(platform, &root);
    let home = home_of(&directory, platform);
    fs::remove_file(home.join("lib/packed.txt")).expect("remove");

    let installed =
        block_on(install(&FakeNetwork { responses }, &root, 21, |_| {})).expect("install");

    assert_eq!(
        fs::read(home.join("lib/packed.txt")).expect("restored"),
        PLAIN_BODY
    );
    assert!(installed.java.is_file());
    let _ = fs::remove_dir_all(&root);
}

/// The record names a digest per file, so a file altered in place is not the
/// one that was installed.
#[test]
fn a_runtime_whose_file_changed_is_downloaded_again() {
    let Some(platform) = platform() else {
        return;
    };
    let root = temp();
    let (directory, responses) = install_once(platform, &root);
    let home = home_of(&directory, platform);
    fs::write(home.join("lib/plain.txt"), b"tampered").expect("tamper");

    let installed =
        block_on(install(&FakeNetwork { responses }, &root, 21, |_| {})).expect("install");

    assert_eq!(
        fs::read(home.join("lib/plain.txt")).expect("restored"),
        PLAIN_BODY
    );
    assert!(installed.java.is_file());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_unsupported_major_is_named() {
    let root = temp();
    let error = block_on(install(
        &FakeNetwork {
            responses: BTreeMap::new(),
        },
        &root,
        7,
        |_| {},
    ))
    .expect_err("unsupported");

    assert!(
        matches!(error, RuntimeError::Unsupported { major: 7 }),
        "{error}"
    );
}

#[test]
fn a_manifest_path_that_leaves_the_runtime_is_refused() {
    assert!(safe_join(Path::new("r"), "../escape").is_none());
    assert!(safe_join(Path::new("r"), "/absolute").is_none());
    assert!(safe_join(Path::new("r"), "bin/java").is_some());
}

#[test]
fn every_component_mapping_is_the_published_one() {
    assert_eq!(component_for(8), Some("jre-legacy"));
    assert_eq!(component_for(17), Some("java-runtime-gamma"));
    assert_eq!(component_for(21), Some("java-runtime-delta"));
    assert_eq!(component_for(25), Some("java-runtime-epsilon"));
    assert_eq!(component_for(7), None);
}
