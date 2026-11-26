// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::{Activity, JavaPanel, Operation, catalog_rows};
use axlauncher_core::storage::DataDir;

#[test]
fn returning_to_a_data_root_does_not_reuse_its_in_flight_result() {
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    let left = DataDir::isolated(left.path()).unwrap();
    let right = DataDir::isolated(right.path()).unwrap();
    let mut panel = JavaPanel::default();
    panel.change_paths(Some(left.clone()));
    panel
        .begin(Activity::SavingProfile("profile".into()))
        .unwrap();
    assert!(panel.active().is_some());

    panel.change_paths(Some(right.clone()));
    assert!(!panel.write_pending("profile"));
    panel.begin(Activity::Scanning).unwrap();
    assert!(panel.take_completed(right.root()).is_some());

    panel.change_paths(Some(left.clone()));
    assert!(panel.busy());
    assert!(panel.active().is_none());
    assert!(panel.write_pending("profile"));
    assert!(!panel.write_pending("another-profile"));
    assert!(panel.take_completed(left.root()).is_none());
    assert!(!panel.busy());
}

#[test]
fn java_panel_actions_reopen_saved_catalog_and_instance_choices() {
    let folder = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(folder.path()).unwrap();
    let executable = java_fixture(folder.path());
    let registered = Operation::Add(executable).run(&data).unwrap();
    let path = registered.manual_paths().next().unwrap().to_owned();
    assert_eq!(registered.default_for(42), Some(path.as_path()));
    assert_eq!(
        catalog_rows(&registered)[path.as_path()].default_major,
        Some(42)
    );
    assert!(registered.preferred().next().is_none());
    let profile = "selected-instance".to_owned();

    Operation::Prefer {
        major: 42,
        path: Some(path.clone()),
    }
    .run(&data)
    .unwrap();
    Operation::Select {
        profile: profile.clone(),
        path: Some(path.clone()),
    }
    .run(&data)
    .unwrap();
    let loaded = Operation::Refresh.run(&data).unwrap();
    assert_eq!(
        loaded.preferred().collect::<Vec<_>>(),
        [(42, path.as_path())]
    );
    assert_eq!(loaded.profile_runtime(&profile).unwrap().major(), 42);

    let automatic = Operation::Select {
        profile: profile.clone(),
        path: None,
    }
    .run(&data)
    .unwrap();
    assert!(automatic.profile_preference(&profile).is_none());
    let removed = Operation::Remove(path).run(&data).unwrap();
    assert!(removed.manual_paths().next().is_none());
    assert!(removed.preferred().next().is_none());
}

fn java_fixture(root: &std::path::Path) -> std::path::PathBuf {
    let home = root.join("jdk");
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(home.join("release"), b"JAVA_VERSION=\"42.0.1\"\n").unwrap();
    let executable = bin.join(if cfg!(windows) { "java.exe" } else { "java" });
    std::fs::write(&executable, b"java fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    executable
}
