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

    assert_eq!(
        job.request
            .game_root
            .native_classifier_archive(Path::new("native.jar"), Some(hash)),
        expected
    );
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

#[test]
fn a_profile_override_is_used_and_checked_against_the_version_requirement() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let job = java_profile_job(&data);
    let id = &job.profile_id;
    let selected = java::test_runtime(&root.path().join("selected"), 42);
    let other = java::test_runtime(&root.path().join("other"), 43);
    let automatic = java::test_runtime(&root.path().join("automatic"), 42);
    java::set_preferred(&data, 42, Some(&automatic)).unwrap();
    java::set_profile_preference(&data, id, Some(&selected)).unwrap();
    let inspection = job.inspect().unwrap();
    assert!(
        inspection
            .java()
            .unwrap()
            .ends_with(selected.strip_prefix(root.path()).unwrap())
    );

    java::set_profile_preference(&data, id, Some(&other)).unwrap();
    let error = job.preview(&Session::offline("Player")).unwrap_err();
    assert!(matches!(
        error,
        FlowError::InvalidJavaPreference {
            required: Some(42),
            ..
        }
    ));

    java::set_profile_preference(&data, id, None).unwrap();
    let inspection = job.inspect().unwrap();
    assert!(
        inspection
            .java()
            .unwrap()
            .ends_with(automatic.strip_prefix(root.path()).unwrap())
    );
}

#[test]
fn a_removed_explicit_java_is_reported_even_when_automatic_java_is_available() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let job = java_profile_job(&data);
    let chosen = java::test_runtime(&root.path().join("chosen"), 42);
    let fallback = java::test_runtime(&root.path().join("fallback"), 42);
    java::set_preferred(&data, 42, Some(&fallback)).unwrap();
    java::set_profile_preference(&data, &job.profile_id, Some(&chosen)).unwrap();
    std::fs::remove_file(chosen).unwrap();

    assert!(matches!(
        job.inspect(),
        Err(FlowError::InvalidJavaPreference { .. })
    ));
    assert!(matches!(
        job.preview(&Session::offline("Player")),
        Err(FlowError::InvalidJavaPreference { .. })
    ));
}

#[test]
fn an_unreadable_java_catalog_is_reported_and_preserved() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let job = java_profile_job(&data);
    let catalog = axlauncher_storage::javas_path(data.root());
    let contents = b"{broken";
    std::fs::write(&catalog, contents).unwrap();

    assert!(matches!(
        job.inspect(),
        Err(FlowError::JavaConfiguration { .. })
    ));
    assert!(matches!(
        job.preview(&Session::offline("Player")),
        Err(FlowError::JavaConfiguration { .. })
    ));
    assert_eq!(std::fs::read(catalog).unwrap(), contents);
}

fn java_profile_job(data: &DataDir) -> LaunchJob {
    let version = "1.21.11";
    profile::create(&data.instances(), "Fixture", Game::new(version)).unwrap();
    let profiles = profile::discover(&data.instances());
    let manifest = data.game().version_document(version);
    std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    std::fs::write(
        manifest,
        r#"{"id":"1.21.11","javaVersion":{"majorVersion":42}}"#,
    )
    .unwrap();
    LaunchJob::for_profile(data, &profiles.instances[0], "Player".into(), None).unwrap()
}
