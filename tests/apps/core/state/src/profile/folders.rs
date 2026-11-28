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

#[test]
fn the_previous_lock_namespace_can_be_an_instance_directory() {
    let root = tempfile::tempdir().unwrap();
    let directory = create(root.path(), "Instance", Game::new("1.21.11")).unwrap();
    let named = root.path().join(".profile-locks");
    fs::rename(directory, &named).unwrap();
    let profile = read(&named).unwrap().unwrap();
    let updated =
        super::super::rename_if_current(&named, profile.id(), "Instance", "Renamed", || true)
            .unwrap();
    assert_eq!(updated.instance.name, "Renamed");
    let held = Document::Profile.lock(&named).unwrap();
    let holding = storage::instance_move_path(&named).unwrap();
    storage::move_instance_folder(&named, &holding).unwrap();
    assert!(check_identity(&holding, profile.id()).is_ok());
    drop(held);
}

#[test]
fn a_replacement_captured_before_recycling_is_restored_untouched() {
    let root = tempfile::tempdir().unwrap();
    let original = create(root.path(), "Original", Game::new("1.21.11")).unwrap();
    let expected = read(&original).unwrap().unwrap();
    let alternate = root.path().join("original moved away");
    fs::rename(&original, &alternate).unwrap();
    fs::create_dir(&original).unwrap();
    let replacement = Profile::new("Replacement", None);
    save(&original, &replacement).unwrap();
    fs::write(original.join("world"), b"replacement contents").unwrap();
    let holding = storage::instance_move_path(&original).unwrap();
    storage::move_instance_folder(&original, &holding).unwrap();

    let error = remove_held(&original, &holding, expected.id()).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        fs::read(original.join("world")).unwrap(),
        b"replacement contents"
    );
    assert_eq!(read(&original).unwrap().unwrap().id(), replacement.id());
    assert!(check_identity(&alternate, expected.id()).is_ok());
    assert!(!holding.exists());
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
