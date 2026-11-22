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
    let mut hash = sha1_smol::Sha1::new();
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
fn fixture(platform: &str) -> (Vec<u8>, BTreeMap<String, Vec<u8>>) {
    let manifest_url = "https://example.test/manifest.json";
    let raw_url = "https://example.test/java";
    let plain_url = "https://example.test/plain";
    let packed_url = "https://example.test/packed";

    let packed = lzma(PLAIN_BODY);
    let manifest = format!(
        r#"{{"files": {{
          "bin/{java_bin}": {{"type":"file","executable":true,"downloads":{{"raw":{{"url":"{raw_url}","sha1":"{raw_sha}"}}}}}},
          "lib/plain.txt": {{"type":"file","executable":false,"downloads":{{"raw":{{"url":"{plain_url}","sha1":"{plain_sha}"}}}}}},
          "lib/packed.txt": {{"type":"file","executable":false,"downloads":{{
            "lzma":{{"url":"{packed_url}","sha1":"{packed_sha}"}},
            "raw":{{"url":"{plain_url}","sha1":"{plain_sha}"}}
          }}}},
          "lib": {{"type":"directory"}}
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
    assert_eq!(
        fs::read(root.join("java-runtime-delta/lib/packed.txt")).expect("packed"),
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

#[test]
fn a_runtime_already_in_place_is_not_downloaded_again() {
    let Some(platform) = platform() else {
        return;
    };
    let root = temp();
    let (_, responses) = fixture(platform);
    let directory = root.join("java-runtime-delta");
    let java = directory.join("bin").join(JAVA_BIN);
    fs::create_dir_all(java.parent().expect("parent")).expect("dir");
    fs::write(&java, RAW_BODY).expect("seed");

    // Only the index is answered: a second manifest or object fetch fails.
    let mut index_only = BTreeMap::new();
    index_only.insert(
        RUNTIME_INDEX.to_string(),
        responses.get(RUNTIME_INDEX).cloned().expect("index"),
    );
    let installed = block_on(install(
        &FakeNetwork {
            responses: index_only,
        },
        &root,
        21,
        |_| {},
    ))
    .expect("install");

    assert_eq!(installed.java, java);
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
