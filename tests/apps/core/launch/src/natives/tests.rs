// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;

use super::*;
use crate::version::{Artifact, LibraryDownloads, OsRule, Rule};

fn target(os: &str, arch: &str) -> Target {
    Target {
        os: os.into(),
        arch: arch.into(),
    }
}

fn artifact(path: &str) -> Artifact {
    Artifact {
        path: path.into(),
        sha1: None,
        sha1_alternatives: Vec::new(),
        size: None,
        url: None,
    }
}

fn allow(os: &str) -> Vec<Rule> {
    vec![Rule {
        action: "allow".into(),
        features: std::collections::BTreeMap::new(),
        os: Some(OsRule {
            name: Some(os.into()),
            arch: None,
        }),
    }]
}

fn native_library(name: &str, path: &str, os: &str) -> Library {
    Library {
        name: name.into(),
        downloads: Some(LibraryDownloads {
            artifact: Some(artifact(path)),
            classifiers: None,
        }),
        natives: None,
        rules: Some(allow(os)),
        url: None,
        include_in_classpath: true,
    }
}

fn ordinary_library(name: &str, path: &str, os: &str) -> Library {
    Library {
        name: name.into(),
        downloads: Some(LibraryDownloads {
            artifact: Some(artifact(path)),
            classifiers: None,
        }),
        natives: None,
        rules: Some(allow(os)),
        url: None,
        include_in_classpath: true,
    }
}

#[derive(Default)]
struct MemoryReader {
    jars: BTreeMap<PathBuf, Vec<(String, Vec<u8>)>>,
}

impl MemoryReader {
    fn with_jar(mut self, path: PathBuf, entries: Vec<(String, Vec<u8>)>) -> Self {
        self.jars.insert(path, entries);
        self
    }
}

impl NativeReader for MemoryReader {
    fn read_jar(&self, path: &Path) -> Result<Vec<(String, Vec<u8>)>, NativeError> {
        self.jars
            .get(path)
            .cloned()
            .ok_or_else(|| NativeError::NotAJar {
                path: path.to_path_buf(),
            })
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("axlauncher-natives-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create scratch directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_cached_classifier_extracts_into_the_shared_version_directory() {
    let temp = TempDir::new("cached-classifier");
    let game_root = temp.0.join("meta");
    let hash = "a9993e364706816aba3e25717850c26c9cd0d89d";
    let archive = axlauncher_storage::native_archive(&game_root, hash).expect("archive path");
    fs::create_dir_all(archive.parent().expect("parent")).expect("cache directory");
    fs::write(&archive, b"abc").expect("cached jar");
    let reader =
        MemoryReader::default().with_jar(archive, vec![("lwjgl.dll".into(), b"native".to_vec())]);

    let library = serde_json::from_value(serde_json::json!({
        "name": "org.lwjgl:lwjgl-platform:2.9.4",
        "natives": {"windows": "natives-windows"},
        "downloads": {"classifiers": {"natives-windows": {
            "path": "org/lwjgl/lwjgl-platform/2.9.4/lwjgl-platform-2.9.4-natives-windows.jar",
            "sha1": hash,
            "size": 3,
        }}},
    }))
    .expect("library");
    let extracted = extract_version(
        &reader,
        &game_root,
        "1.12.2",
        &[library],
        &target("windows", "x86_64"),
    )
    .expect("extract cached jar");

    assert_eq!(
        extracted.files,
        [game_root.join("natives/1.12.2/lwjgl.dll")]
    );
}

#[test]
fn new_shape_natives_follow_the_target_rules() {
    let libraries = [
        native_library(
            "org.lwjgl:lwjgl:3.3.1:natives-linux",
            "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-linux.jar",
            "linux",
        ),
        native_library(
            "org.lwjgl:lwjgl:3.3.1:natives-windows",
            "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar",
            "windows",
        ),
        ordinary_library(
            "org.lwjgl:lwjgl:3.3.1",
            "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar",
            "linux",
        ),
    ];

    assert_eq!(
        entries(&libraries, &target("linux", "x86_64")),
        vec![NativeEntry {
            path: "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-linux.jar".into(),
        }]
    );
    assert_eq!(
        entries(&libraries, &target("windows", "x86_64")),
        vec![NativeEntry {
            path: "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar".into(),
        }]
    );
}

#[test]
fn old_shape_natives_substitute_the_target_arch() {
    let library = Library {
        name: "org.lwjgl.lwjgl:lwjgl-platform:2.9.4".into(),
        downloads: Some(LibraryDownloads {
            artifact: None,
            classifiers: Some(BTreeMap::from([
                (
                    "natives-windows-32".into(),
                    artifact("lwjgl-platform-2.9.4-natives-windows-32.jar"),
                ),
                (
                    "natives-windows-64".into(),
                    artifact("lwjgl-platform-2.9.4-natives-windows-64.jar"),
                ),
            ])),
        }),
        natives: Some(BTreeMap::from([(
            "windows".into(),
            "natives-windows-${arch}".into(),
        )])),
        rules: None,
        url: None,
        include_in_classpath: true,
    };

    assert_eq!(
        entries(std::slice::from_ref(&library), &target("windows", "x86_64")),
        vec![NativeEntry {
            path: "lwjgl-platform-2.9.4-natives-windows-64.jar".into(),
        }]
    );
    assert_eq!(
        entries(std::slice::from_ref(&library), &target("windows", "x86")),
        vec![NativeEntry {
            path: "lwjgl-platform-2.9.4-natives-windows-32.jar".into(),
        }]
    );
}

#[test]
fn only_native_files_are_extracted_and_directories_are_skipped() {
    let root = TempDir::new("extensions");
    let jars_root = root.0.join("libraries");
    let destination = root.0.join("natives");
    let jar = jars_root.join("natives.jar");
    let reader = MemoryReader::default().with_jar(
        jar,
        vec![
            ("lwjgl.dll".into(), b"dll".to_vec()),
            ("liblwjgl.so".into(), b"so".to_vec()),
            ("liblwjgl.dylib".into(), b"dylib".to_vec()),
            ("liblwjgl.jnilib".into(), b"jnilib".to_vec()),
            ("META-INF/".into(), Vec::new()),
            ("META-INF/MANIFEST.MF".into(), b"manifest".to_vec()),
            ("notes.txt".into(), b"text".to_vec()),
        ],
    );

    let extracted = extract(&reader, &jars_root, &[entry("natives.jar")], &destination)
        .expect("extraction succeeds");

    assert_eq!(
        extracted.files,
        vec![
            destination.join("lwjgl.dll"),
            destination.join("liblwjgl.so"),
            destination.join("liblwjgl.dylib"),
            destination.join("liblwjgl.jnilib"),
        ]
    );
    assert_eq!(
        fs::read(destination.join("lwjgl.dll")).expect("read extracted dll"),
        b"dll"
    );
    assert_eq!(
        extracted.skipped,
        vec![
            "META-INF/".to_string(),
            "META-INF/MANIFEST.MF".to_string(),
            "notes.txt".to_string(),
        ]
    );
    assert!(!destination.join("META-INF").exists());
}

#[test]
fn entries_that_escape_the_destination_are_skipped() {
    let root = TempDir::new("zip-slip");
    let jars_root = root.0.join("libraries");
    let destination = root.0.join("natives");
    let jar = jars_root.join("natives.jar");
    let outside = root.0.join("escaped.dll");
    let reader = MemoryReader::default().with_jar(
        jar,
        vec![
            ("../escaped.dll".into(), b"escape".to_vec()),
            ("a/../../escaped.dll".into(), b"escape".to_vec()),
            ("/absolute.dll".into(), b"escape".to_vec()),
            ("nested/ok.dll".into(), b"kept".to_vec()),
        ],
    );

    let extracted = extract(&reader, &jars_root, &[entry("natives.jar")], &destination)
        .expect("extraction succeeds");

    assert!(!outside.exists(), "a native escaped the destination");
    assert_eq!(
        extracted.files,
        vec![destination.join("nested").join("ok.dll")]
    );
    assert_eq!(extracted.skipped.len(), 3);
}

#[test]
fn a_nested_directory_is_preserved_inside_the_destination() {
    let root = TempDir::new("nested");
    let jars_root = root.0.join("libraries");
    let destination = root.0.join("natives");
    let jar = jars_root.join("natives.jar");
    let reader = MemoryReader::default()
        .with_jar(jar, vec![("linux/x64/liblwjgl.so".into(), b"so".to_vec())]);

    extract(&reader, &jars_root, &[entry("natives.jar")], &destination)
        .expect("extraction succeeds");

    assert!(
        destination
            .join("linux")
            .join("x64")
            .join("liblwjgl.so")
            .is_file()
    );
}

#[test]
fn extraction_creates_a_missing_destination() {
    let root = TempDir::new("missing-destination");
    let jars_root = root.0.join("libraries");
    let destination = root.0.join("missing").join("natives");
    assert!(!destination.exists());
    let reader = MemoryReader::default().with_jar(
        jars_root.join("natives.jar"),
        vec![("lwjgl.dll".into(), b"dll".to_vec())],
    );

    extract(&reader, &jars_root, &[entry("natives.jar")], &destination)
        .expect("extraction succeeds");

    assert!(destination.is_dir());
    assert_eq!(
        fs::read(destination.join("lwjgl.dll")).expect("read extracted dll"),
        b"dll"
    );
}

#[test]
fn extraction_replaces_an_existing_native() {
    let root = TempDir::new("replace");
    let jars_root = root.0.join("libraries");
    let destination = root.0.join("natives");
    let jar = jars_root.join("natives.jar");
    let first = MemoryReader::default()
        .with_jar(jar.clone(), vec![("lwjgl.dll".into(), b"first".to_vec())]);
    let replacement =
        MemoryReader::default().with_jar(jar, vec![("lwjgl.dll".into(), b"second".to_vec())]);

    extract(&first, &jars_root, &[entry("natives.jar")], &destination)
        .expect("first extraction succeeds");
    extract(
        &replacement,
        &jars_root,
        &[entry("natives.jar")],
        &destination,
    )
    .expect("second extraction succeeds");

    assert_eq!(
        fs::read(destination.join("lwjgl.dll")).expect("read replaced dll"),
        b"second"
    );
}

#[test]
fn a_file_that_is_not_a_jar_is_reported() {
    let root = TempDir::new("not-a-jar");
    let jars_root = root.0.join("libraries");
    let destination = root.0.join("natives");
    let jar = jars_root.join("broken.jar");
    let reader = MemoryReader::default();

    let error = extract(&reader, &jars_root, &[entry("broken.jar")], &destination)
        .expect_err("a non-jar fails");

    match error {
        NativeError::NotAJar { path } => assert_eq!(path, jar),
        other => panic!("expected NotAJar, got {other:?}"),
    }
}

fn entry(path: &str) -> NativeEntry {
    NativeEntry { path: path.into() }
}
