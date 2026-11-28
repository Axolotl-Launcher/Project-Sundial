// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::super::{Game, Profile, create, save};
use super::*;

#[test]
fn deleting_a_replaced_profile_preserves_the_new_folder() {
    let root = tempfile::tempdir().unwrap();
    let paths = DataDir::isolated(root.path()).unwrap();
    let directory = create(
        paths.path(DataPath::Profiles),
        "Instance",
        Game::new("1.21.11"),
    )
    .unwrap();
    let previous = read(&directory).unwrap().unwrap();
    let replacement = Profile::new("Replacement", None);
    save(&directory, &replacement).unwrap();

    let error = delete_folder(&paths, &directory, previous.id()).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(read(&directory).unwrap().unwrap().id(), replacement.id());
}

#[test]
fn deleting_an_instance_outside_the_active_profiles_root_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let paths = DataDir::isolated(root.path()).unwrap();
    let directory = create(root.path(), "Outside", Game::new("1.21.11")).unwrap();
    let profile = read(&directory).unwrap().unwrap();

    let error = delete_folder(&paths, &directory, profile.id()).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(directory.is_dir());
}

#[test]
fn locking_profile_writers_still_allows_the_folder_to_move() {
    let root = tempfile::tempdir().unwrap();
    let directory = create(root.path(), "Instance", Game::new("1.21.11")).unwrap();
    let _lock = Document::Profile.lock(&directory).unwrap();
    let moved = root.path().join("moved");

    fs::rename(directory, &moved).unwrap();

    assert!(read(&moved).unwrap().is_some());
}

#[test]
fn discovery_ignores_the_lock_for_an_instance_named_like_its_marker() {
    let root = tempfile::tempdir().unwrap();
    let directory = create(root.path(), "Instance", Game::new("1.21.11")).unwrap();
    let renamed = root.path().join(".axlprofile");
    fs::rename(directory, &renamed).unwrap();
    let _lock = Document::Profile.lock(&renamed).unwrap();

    let profiles = super::super::discover(root.path());

    assert_eq!(profiles.instances.len(), 1);
    assert_eq!(profiles.instances[0].directory, renamed);
    assert!(profiles.issues.is_empty());
}

#[cfg(unix)]
#[test]
fn deleting_a_link_to_a_profile_preserves_its_target() {
    let root = tempfile::tempdir().unwrap();
    let paths = DataDir::isolated(root.path()).unwrap();
    let instances = paths.path(DataPath::Profiles);
    fs::create_dir_all(instances).unwrap();
    let directory = create(root.path(), "Outside", Game::new("1.21.11")).unwrap();
    let profile = read(&directory).unwrap().unwrap();
    let link = instances.join("linked");
    std::os::unix::fs::symlink(&directory, &link).unwrap();

    let error = delete_folder(&paths, &link, profile.id()).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(directory.is_dir());
    assert!(link.is_symlink());
}
