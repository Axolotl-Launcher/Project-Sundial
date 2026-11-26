// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_state::profile::{self, Game, InstallStatus, Profile, StagedProfile};
use axlauncher_storage::{Directory, SystemCredentials};

use super::*;

#[test]
fn a_launch_job_retains_the_directory_snapshot_used_for_installation() {
    let root = tempfile::tempdir().unwrap();
    let mut paths = DataDir::isolated(root.path()).unwrap();
    let install_cache = root.path().join("first-cache");
    paths
        .set_directory(Directory::AppCache, Some(&install_cache))
        .unwrap();
    profile::create(&paths.instances(), "Fixture", Game::new("1.21.11")).unwrap();
    let discovered = profile::discover(&paths.instances());
    let entry = &discovered.instances[0];
    let job = LaunchJob::for_profile(&paths, entry, "Player".into(), None).unwrap();
    let hash = "a9993e364706816aba3e25717850c26c9cd0d89d";
    let expected = install_cache
        .join("minecraft-natives")
        .join(format!("{hash}.jar"));

    paths
        .set_directory(Directory::AppCache, Some(&root.path().join("second-cache")))
        .unwrap();

    assert_eq!(job.request.game_root.native_archive(hash), Some(expected));
    assert_ne!(job.request.game_root, paths.game());
}

#[test]
fn a_converted_profile_keeps_its_isolated_runtime_after_publication() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let instances = data.instances();
    let staged = StagedProfile::new(&instances).unwrap();
    let game = Game::new("1.20.1").with_isolated_runtime();
    let version = "axolotl-converted";
    let runtime = GameDir::isolated(staged.directory());
    std::fs::create_dir(runtime.root()).unwrap();
    std::fs::write(runtime.root().join("kept"), b"converted files").unwrap();
    assert!(profile::discover(&instances).instances.is_empty());

    let directory = staged
        .publish_installed(Profile::new("Converted", Some(game)), version)
        .unwrap();
    let discovered = profile::discover(&instances);
    let entry = &discovered.instances[0];
    let job = LaunchJob::for_profile(&data, entry, "Player".into(), None).unwrap();
    assert_eq!(entry.profile.install.status, InstallStatus::Ready);
    assert_eq!(job.version_id(), version);
    assert_eq!(job.game_root(), GameDir::isolated(&directory).root());
    assert_eq!(
        std::fs::read(job.game_root().join("kept")).unwrap(),
        b"converted files"
    );
    assert!(super::super::installation::install_key(entry.profile.game().unwrap()).is_none());
}

#[test]
fn a_profile_without_game_files_needs_install_and_starts_nothing() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let game = Game::new("1.21.11");
    profile::create(&data.instances(), "Fixture", game).unwrap();
    let discovered = profile::discover(&data.instances());
    let job = LaunchJob::for_profile(&data, &discovered.instances[0], "Player".into(), None)
        .expect("vanilla profile has a version");
    assert_eq!(job.game_root(), data.game().root());
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
