// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "axlauncher-storage-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn write_atomic_replaces_the_target_and_leaves_no_temporary() {
    let directory = TempDir::new();
    let target = directory.0.join("settings.json");
    fs::write(&target, b"old").expect("seed");

    write_atomic(&target, b"new").expect("write");

    assert_eq!(fs::read(&target).expect("read"), b"new");
    assert!(!directory.0.join("settings.json.tmp").exists());
}

#[test]
fn write_atomic_creates_missing_parents() {
    let directory = TempDir::new();
    let target = directory.0.join("nested").join("file.json");

    write_atomic(&target, b"data").expect("write");

    assert_eq!(fs::read(&target).expect("read"), b"data");
}

#[test]
fn an_asset_object_lives_under_its_two_character_prefix() {
    let layout = DataDir::new("root");

    assert_eq!(
        layout.asset_object("ab12").expect("hash with a prefix"),
        Path::new("root/assets/objects/ab/ab12")
    );
    assert_eq!(layout.asset_object("a"), None);
}

#[test]
fn each_version_keeps_its_own_natives_and_game_files() {
    let layout = DataDir::new("root");

    assert_eq!(layout.version("1.20.1"), Path::new("root/versions/1.20.1"));
    assert_eq!(
        layout.version_natives("1.20.1"),
        Path::new("root/natives/1.20.1")
    );
    assert_ne!(
        layout.version_natives("1.20.1"),
        layout.version_natives("1.20.2")
    );
}

#[test]
fn a_virtual_asset_tree_is_named_after_its_index() {
    let layout = DataDir::new("root");

    assert_eq!(
        layout.virtual_assets("legacy"),
        Path::new("root/assets/virtual/legacy")
    );
}
