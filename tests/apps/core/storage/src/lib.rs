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
        layout.version_path("1.20.1", VersionPath::Document),
        Path::new("root/meta/versions/1.20.1/1.20.1.json")
    );
    assert_eq!(
        layout.version_path("1.20.1", VersionPath::Natives),
        Path::new("root/meta/natives/1.20.1")
    );
    assert_ne!(
        layout.version_path("1.20.1", VersionPath::Natives),
        layout.version_path("1.20.2", VersionPath::Natives)
    );
}

#[test]
fn a_virtual_asset_tree_is_named_after_its_index() {
    let layout = GameDir::standalone("root/meta");

    assert_eq!(
        layout.asset_path(AssetPath::Virtual("legacy")),
        Path::new("root/meta/assets/virtual/legacy")
    );
}

#[test]
fn profiles_and_runtime_files_use_the_managed_layout() {
    let root = TempDir::new();
    let layout = DataDir::isolated(&root.0).unwrap();
    let game = layout.game();
    assert_eq!(layout.path(DataPath::Profiles), root.0.join("profiles"));
    assert_eq!(
        game.version_path("client.xml", VersionPath::LogConfig),
        root.0.join("meta/log_configs/client.xml")
    );
}

#[test]
fn classifier_archives_use_the_shared_cache_only_with_a_usable_hash() {
    let root = TempDir::new();
    let layout = DataDir::isolated(&root.0).unwrap();
    let game = layout.game();
    let hash = "0123456789012345678901234567890123456789";
    let relative = Path::new("org/example/native.jar");
    let maven_path = game.libraries().join(relative);
    assert_eq!(
        game.native_classifier_archive(relative, Some(hash)),
        layout
            .path(DataPath::Cache)
            .join("minecraft-natives")
            .join(format!("{hash}.jar"))
    );
    assert_eq!(
        game.native_classifier_archive(relative, Some("../escape")),
        maven_path
    );
    assert_eq!(game.native_classifier_archive(relative, None), maven_path);
    let isolated = GameDir::isolated(layout.path(DataPath::Profiles));
    assert_eq!(
        isolated.native_classifier_archive(relative, Some(hash)),
        isolated.libraries().join(relative)
    );
}

#[test]
fn simultaneous_launches_share_the_game_lock_and_exclude_installation() {
    let root = TempDir::new();
    let game = GameDir::standalone(root.0.join("game"));
    let first = game.lock(GameAccess::Launch).unwrap();
    let second = game.lock(GameAccess::Launch).unwrap();
    let busy = game.lock(GameAccess::Install).unwrap_err();
    assert_eq!(io::Error::from(busy).kind(), io::ErrorKind::WouldBlock);
    drop(first);
    assert!(game.lock(GameAccess::Install).is_err());
    drop(second);
    let install = game.lock(GameAccess::Install).unwrap();
    assert!(game.lock(GameAccess::Launch).is_err());
    drop(install);
    assert!(game.lock(GameAccess::Launch).is_ok());
}

#[test]
fn locking_a_missing_profile_does_not_recreate_its_directory() {
    let root = TempDir::new();
    let instance = root.0.join("missing");
    let error = Document::Profile.lock(&instance).unwrap_err();
    assert_eq!(io::Error::from(error).kind(), io::ErrorKind::NotFound);
    assert!(!instance.exists());
}

#[test]
fn profile_locks_serialize_one_instance_without_blocking_its_neighbors() {
    let root = TempDir::new();
    let first = root.0.join("first");
    let second = root.0.join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let held = Document::Profile.lock(&first).unwrap();
    let lock_directory = root.0.join(".profile-locks/files");

    let first_lock = fs::File::open(lock_directory.join("first")).unwrap();
    assert!(matches!(
        first_lock.try_lock(),
        Err(fs::TryLockError::WouldBlock)
    ));

    let (send, receive) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || send.send(Document::Profile.lock(&second)));
    let locked = receive.recv_timeout(std::time::Duration::from_secs(5));
    drop(held);
    writer.join().unwrap().unwrap();
    let _second_lock = locked
        .expect("the neighbor must lock before release")
        .unwrap();
}
