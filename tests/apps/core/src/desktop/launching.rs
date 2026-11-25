// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_state::profile::{self, Game};
use axlauncher_storage::SystemCredentials;

use super::*;

#[test]
fn a_profile_without_game_files_needs_install_and_starts_nothing() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::new(root.path());
    let game = Game::new("1.21.11");
    profile::create(&data.instances(), "Fixture", game).unwrap();
    let discovered = profile::discover(&data.instances());
    let job = LaunchJob::for_profile(root.path(), &discovered.instances[0], "Player".into(), None)
        .expect("vanilla profile has a version");
    assert_eq!(job.game_root(), data.meta());
    assert_eq!(job.version_id(), "1.21.11");
    assert!(job.instance_directory().starts_with(data.instances()));

    let failure = job
        .run(
            Player::<SystemCredentials>::Offline(Session::offline("Player")),
            DownloadOptions::from_settings(None),
            |_| panic!("offline launch must not resolve an online account"),
            || panic!("missing files must not start a process"),
        )
        .unwrap_err();
    assert!(
        matches!(failure, LaunchFailure::NeedsInstall(reason) if reason.starts_with("Install game files to continue."))
    );
}
