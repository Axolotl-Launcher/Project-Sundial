use axlauncher_core::state::profile::{self, Game};
use axlauncher_core::storage::Directory;

use super::*;

#[test]
fn completion_refreshes_the_list_after_reselecting_the_default_root() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("original");
    let worker = DataDir::isolated(&root).unwrap();
    let mut current = worker.clone();
    current
        .set_directory(Directory::AppData, Some(&fixture.path().join("other")))
        .unwrap();
    current
        .set_directory(Directory::AppData, Some(&root))
        .unwrap();
    let mut state = InstancesState::new(discover(&current.instances()));
    let installed =
        profile::create(&worker.instances(), "Installed", Game::new("1.21.11")).unwrap();

    assert!(state.refresh_after_install(Some(&current), &worker));
    assert!(state.find(&installed).is_some());
}

#[test]
fn completion_leaves_another_instance_directory_untouched() {
    let fixture = tempfile::tempdir().unwrap();
    let worker = DataDir::isolated(fixture.path()).unwrap();
    let mut current = worker.clone();
    current
        .set_directory(Directory::UserData, Some(&fixture.path().join("other")))
        .unwrap();
    let selected = profile::create(&current.instances(), "Current", Game::new("1.21.11")).unwrap();
    let mut state = InstancesState::new(discover(&current.instances()));
    let installed =
        profile::create(&worker.instances(), "Installed", Game::new("1.21.11")).unwrap();

    assert!(!state.refresh_after_install(Some(&current), &worker));
    assert!(state.find(&installed).is_none());
    assert_eq!(state.selected().unwrap().directory, selected);
}
