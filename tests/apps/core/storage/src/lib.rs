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

#[cfg(unix)]
#[test]
fn console_output_refuses_a_redirected_logs_directory() {
    let instance = TempDir::new();
    let other = TempDir::new();
    std::os::unix::fs::symlink(&other.0, instance.0.join("logs")).unwrap();
    assert!(open_instance_console_log(&instance.0).is_err());
    assert_eq!(fs::read_dir(&other.0).unwrap().count(), 0);
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
    let layout = GameDir::standalone("root/meta");

    assert_eq!(
        layout.asset_object("ab12").expect("hash with a prefix"),
        Path::new("root/meta/assets/objects/ab/ab12")
    );
    assert_eq!(layout.asset_object("a"), None);
}

#[test]
fn each_version_keeps_its_own_natives_and_game_files() {
    let layout = GameDir::standalone("root/meta");

    assert_eq!(
        layout.version_document("1.20.1"),
        Path::new("root/meta/versions/1.20.1/1.20.1.json")
    );
    assert_eq!(
        layout.version_natives("1.20.1"),
        Path::new("root/meta/natives/1.20.1")
    );
    assert_ne!(
        layout.version_natives("1.20.1"),
        layout.version_natives("1.20.2")
    );
}

#[test]
fn a_virtual_asset_tree_is_named_after_its_index() {
    let layout = GameDir::standalone("root/meta");

    assert_eq!(
        layout.virtual_assets("legacy"),
        Path::new("root/meta/assets/virtual/legacy")
    );
}

#[test]
fn profiles_and_runtime_files_use_the_managed_layout() {
    let root = TempDir::new();
    let layout = DataDir::isolated(&root.0).unwrap();
    let game = layout.game();
    assert_eq!(layout.instances(), root.0.join("profiles"));
    assert_eq!(
        game.log_config("client.xml"),
        root.0.join("meta/log_configs/client.xml")
    );
    let hash = "0123456789012345678901234567890123456789";
    assert_eq!(
        game.native_archive(hash),
        Some(
            layout
                .caches()
                .join("minecraft-natives")
                .join(format!("{hash}.jar"))
        )
    );
    assert_eq!(game.native_archive("../escape"), None);
    assert_eq!(
        GameDir::isolated(&layout.instances()).native_archive(hash),
        None
    );
}
