// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_state::profile::{self, Game, InstallStatus, Profile, StagedProfile};
use axlauncher_storage::Directory;

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
            Player::Offline(Session::offline("Player")),
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
fn a_profile_override_takes_precedence_even_with_a_different_java_major() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let job = java_profile_job(&data);
    let id = &job.profile_id;
    let selected = java::test_runtime(&root.path().join("selected"), 43);
    let automatic = java::test_runtime(&root.path().join("automatic"), 42);
    java::add(&data, &automatic).unwrap();
    java::add(&data, &selected).unwrap();
    java::set_profile_preference(&data, id, Some(&selected)).unwrap();
    assert_selected_java(&job, &selected);

    java::set_profile_preference(&data, id, None).unwrap();
    assert_selected_java(&job, &automatic);
}

#[test]
fn a_missing_profile_java_uses_automatic_selection_until_it_returns() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let job = java_profile_job(&data);
    let home = root.path().join("chosen");
    let chosen = java::test_runtime(&home, 42);
    let fallback = java::test_runtime(&root.path().join("fallback"), 42);
    java::add(&data, &fallback).unwrap();
    java::add(&data, &chosen).unwrap();
    java::set_profile_preference(&data, &job.profile_id, Some(&chosen)).unwrap();
    std::fs::remove_file(&chosen).unwrap();

    assert_selected_java(&job, &fallback);

    java::test_runtime(&home, 42);
    assert_selected_java(&job, &chosen);
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

#[test]
fn desktop_preview_applies_profile_then_manifest_heap_settings() {
    use axlauncher_domain::memory::MaximumHeap;

    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let fixture = java_profile_job(&data);
    let runtime = java::test_runtime(&root.path().join("runtime-fixture"), 42);
    java::add(&data, &runtime).unwrap();
    let mut settings = SettingsData::default();
    settings.set_memory(MemoryAllocation::Manual(MaximumHeap::new(4096).unwrap()));
    let preview = || {
        let discovered = profile::discover(&data.instances());
        let job = LaunchJob::for_profile(
            &data,
            &discovered.instances[0],
            "Player".into(),
            Some(&settings),
        )
        .unwrap();
        let command = job.preview(&Session::offline("Player")).unwrap();
        command
            .args
            .into_iter()
            .filter(|arg| arg.starts_with("-Xms") || arg.starts_with("-Xmx"))
            .collect::<Vec<_>>()
    };

    assert_eq!(preview(), ["-Xmx4096m"]);
    let heap = MaximumHeap::new(6144).unwrap();
    profile::set_memory(
        fixture.instance_directory(),
        &fixture.profile_id,
        Some(heap),
    )
    .unwrap();
    assert_eq!(preview(), ["-Xmx6144m"]);
    let release = runtime.parent().unwrap().parent().unwrap().join("release");
    std::fs::write(release, "JAVA_VERSION=\"42.0.1\"\nOS_ARCH=\"i686\"\n").unwrap();
    assert_eq!(preview(), ["-Xmx1024m"]);
    assert_eq!(
        profile::read(fixture.instance_directory())
            .unwrap()
            .unwrap()
            .memory(),
        Some(heap)
    );
    std::fs::write(
        data.game().version_document(fixture.version_id()),
        r#"{"id":"1.21.11","javaVersion":{"majorVersion":42},"arguments":{"jvm":["-Xmx512m"]}}"#,
    )
    .unwrap();
    assert_eq!(preview(), ["-Xmx512m"]);
}

#[test]
fn an_optifine_only_profile_uses_the_version_workload() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let game = Game::new("1.21.11").with_loader("OptiFine", None);
    let directory = profile::create(&data.instances(), "OptiFine", game).unwrap();
    profile::mark_installed(&directory, "optifine-test").unwrap();
    let profiles = profile::discover(&data.instances());
    let job = LaunchJob::for_profile(&data, &profiles.instances[0], "Player".into(), None).unwrap();
    assert!(matches!(
        job.configured_launch().unwrap().1.heap,
        Some(HeapPolicy::Automatic { enabled_mods: None })
    ));
}

#[test]
fn an_automatic_job_rescans_enabled_mods_for_each_launch_request() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let game = Game::new("1.21.11").with_loader("fabric", None);
    let directory = profile::create(&data.instances(), "Modded", game).unwrap();
    profile::mark_installed(&directory, "fabric-test").unwrap();
    let profiles = profile::discover(&data.instances());
    let job = LaunchJob::for_profile(&data, &profiles.instances[0], "Player".into(), None).unwrap();
    assert!(matches!(
        job.configured_launch().unwrap().1.heap,
        Some(HeapPolicy::Automatic {
            enabled_mods: Some(0)
        })
    ));

    let mods = directory.join("mods");
    std::fs::create_dir(&mods).unwrap();
    std::fs::write(mods.join("enabled.jar"), b"mod").unwrap();
    std::fs::write(mods.join("disabled.jar.disabled"), b"mod").unwrap();
    assert!(matches!(
        job.configured_launch().unwrap().1.heap,
        Some(HeapPolicy::Automatic {
            enabled_mods: Some(1)
        })
    ));
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
    std::fs::write(data.game().client_jar(version), b"client").unwrap();
    LaunchJob::for_profile(data, &profiles.instances[0], "Player".into(), None).unwrap()
}

fn assert_selected_java(job: &LaunchJob, expected: &Path) {
    use axlauncher_launch::JavaRuntime;

    let inspection = job.inspect().unwrap();
    let executable = inspection.java().unwrap();
    assert_eq!(
        JavaRuntime::inspect(executable).unwrap(),
        JavaRuntime::inspect(expected).unwrap()
    );
    let command = job.preview(&Session::offline("Player")).unwrap();
    assert_eq!(Path::new(&command.program), executable);
}
