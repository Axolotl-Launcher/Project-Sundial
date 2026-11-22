// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use axlauncher_spi::LoaderFile;

fn profile(id: &str) -> LoaderProfile {
    LoaderProfile {
        version_id: id.into(),
        document: br#"{"id":"x","inheritsFrom":"1.21"}"#.to_vec(),
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
    let placed = place(&root, &profile("fabric-loader-0.20.1-1.21.11")).expect("place");

    assert_eq!(
        placed.document,
        root.join("versions/fabric-loader-0.20.1-1.21.11/fabric-loader-0.20.1-1.21.11.json")
    );
    assert!(placed.document.is_file());
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

    let placed = place(&root, &profile).expect("place");

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
        "libraries//double.jar",
        "./libraries/file.jar",
    ] {
        let mut profile = profile("loader-1");
        profile.files = vec![LoaderFile {
            path: path.into(),
            bytes: Vec::new(),
        }];

        let error = place(&root, &profile).expect_err(path);
        assert!(matches!(error, PlaceError::InvalidPath { .. }), "{path}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_id_that_cannot_be_a_directory_is_refused() {
    let root = temp();
    for id in ["", "a/b", "a\\b", "../x", &"x".repeat(MAX_ID_LEN + 1)] {
        let error = place(&root, &profile(id)).expect_err(id);
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
