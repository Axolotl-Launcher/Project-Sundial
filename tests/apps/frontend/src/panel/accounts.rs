use axlauncher_core::desktop::accounts::{AccountId, LauncherAccount};

use super::*;

#[test]
fn an_empty_anonymous_launcher_uses_the_temporary_player() {
    let launcher = LauncherId::new("b50ad385-829d-3141-a216-7e7d7539ba7f".parse().unwrap());
    let snapshot = AccountSnapshot::new(
        vec![LauncherAccount::new(launcher, None)],
        launcher,
        vec![],
        None,
    );

    let player = launch_player(Some(&snapshot), None, None, || {
        Session::offline("Temporary")
    });
    assert!(matches!(player, Player::Offline(_)));
}

#[test]
fn a_saved_offline_player_keeps_its_launcher_scope_at_launch() {
    let launcher = LauncherId::new("ac623055-ddf7-4b0c-bf84-add27126654f".parse().unwrap());
    let id = AccountId::Offline("b50ad385-829d-3141-a216-7e7d7539ba7f".parse().unwrap());
    let snapshot = AccountSnapshot::new(
        vec![LauncherAccount::new(launcher, Some("Local".into()))],
        launcher,
        vec![Account::new(id, "Saved".into(), None, None)],
        Some(id),
    );
    let root = tempfile::tempdir().unwrap();
    let service = DesktopAccounts::new(DataDir::isolated(root.path()).unwrap());
    let player = launch_player(Some(&snapshot), Some(&service), None, || {
        panic!("saved identities must not silently become temporary players")
    });
    assert!(
        matches!(player, Player::Saved { key, .. } if key.launcher() == launcher && key.id() == id)
    );
}
