use std::sync::atomic::{AtomicBool, Ordering};

use axlauncher_core::desktop::{
    instance_settings::{self, EditCancellation, EditError, InstanceTarget},
    java,
    launching::{LaunchFailure, LaunchJob, Player},
    sources::DownloadOptions,
};
use axlauncher_domain::memory::{MaximumHeap, MemoryAllocation};
use axlauncher_launch::plan::Session;
use axlauncher_state::{self as state, profile};
use axlauncher_storage::VersionPath;
use axlauncher_worker::instance_settings::{
    Edit, GlobalHeap, InstanceHeap, InvalidEdit, Reply, ValidatedEdit,
};

use super::*;

fn allocation(mib: u32) -> MemoryAllocation {
    MemoryAllocation::Manual(MaximumHeap::new(mib).unwrap())
}

fn original(fixture: &Fixture) -> InstanceTarget {
    let profiles = fixture.data.path(DataPath::Profiles);
    profile::create(profiles, "Fixture", profile::Game::new("1.21.11")).unwrap();
    target(&fixture.data)
}

fn target(data: &DataDir) -> InstanceTarget {
    InstanceTarget::new(
        &profile::discover(data.path(DataPath::Profiles)).instances[0],
        data,
    )
}

fn instance_memory(data: &DataDir) -> Option<MaximumHeap> {
    profile::read(target(data).directory())
        .unwrap()
        .unwrap()
        .memory()
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn packaged_settings_edits_preserve_scope_and_do_not_affect_launch() {
    let fixture = Fixture::new();
    fixture.images.configure();
    let instance = original(&fixture);
    startup_inspection_preview_and_offline_launch_do_not_start_settings(&fixture, &instance);
    settings_worker_failure_does_not_write_and_retry_respects_overridden_roots(&fixture, &instance);
    settings_edits_keep_inheritance_explicit_and_rename_in_place(&fixture, &instance);
    Images::begin_shutdown().join().unwrap();
}

fn settings_edits_keep_inheritance_explicit_and_rename_in_place(
    fixture: &Fixture,
    instance: &InstanceTarget,
) {
    let marker = Document::Profile.path(instance.directory());
    let original = fs::read(&marker).unwrap();
    let root = fixture.data.path(DataPath::Root);
    assert!(!fixture.has_extracted_libraries());
    for (baseline, choice, expected) in [
        (
            MemoryAllocation::Automatic,
            GlobalHeap::ManualMib(16),
            allocation(16),
        ),
        (
            allocation(16),
            GlobalHeap::ManualMib(2048),
            allocation(2048),
        ),
    ] {
        assert_eq!(
            instance_settings::set_global_heap(
                &fixture.data,
                baseline,
                choice,
                &EditCancellation::default(),
            )
            .unwrap(),
            expected
        );
        assert_eq!(fs::read(&marker).unwrap(), original);
    }
    assert_eq!(
        state::load_settings(root).unwrap().data.memory(),
        allocation(2048)
    );
    instance_settings::set_instance_heap(
        &fixture.data,
        instance,
        InstanceHeap::ManualMib(2048),
        &EditCancellation::default(),
    )
    .unwrap();
    instance_settings::set_global_heap(
        &fixture.data,
        allocation(2048),
        GlobalHeap::ManualMib(4096),
        &EditCancellation::default(),
    )
    .unwrap();
    assert_eq!(instance_memory(&fixture.data), MaximumHeap::new(2048));
    reset_and_rename(fixture, instance.directory());
    fixture.wait_for_cleanup();
}

fn reset_and_rename(fixture: &Fixture, directory: &Path) {
    let current = target(&fixture.data);
    instance_settings::set_instance_heap(
        &fixture.data,
        &current,
        InstanceHeap::Inherit,
        &EditCancellation::default(),
    )
    .unwrap();
    assert!(instance_memory(&fixture.data).is_none());
    assert!(
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(Document::Profile.path(directory)).unwrap()
        )
        .unwrap()
        .get("memory")
        .is_none()
    );
    let renamed = instance_settings::rename(
        &fixture.data,
        &current,
        "  New / 雪  ",
        &EditCancellation::default(),
    )
    .unwrap();
    assert_eq!(renamed.id(), current.id());
    assert_eq!(renamed.instance.name, "New / 雪");
    assert_eq!(target(&fixture.data).directory(), directory);
}

fn settings_worker_failure_does_not_write_and_retry_respects_overridden_roots(
    fixture: &Fixture,
    baseline: &InstanceTarget,
) {
    let mut other = DataDir::isolated(fixture.root.path().join("second root")).unwrap();
    let profiles = fixture.root.path().join("custom profiles");
    let cache = fixture.root.path().join("custom cache");
    other
        .set_directory(Directory::UserData, Some(&profiles))
        .unwrap();
    other
        .set_directory(Directory::AppCache, Some(&cache))
        .unwrap();
    profile::create(&profiles, "Other", profile::Game::new("1.21.11")).unwrap();
    let instance = target(&other);
    let marker = Document::Profile.path(instance.directory());
    let original = fs::read(&marker).unwrap();
    let blocker = cache.join("workers");
    fs::create_dir_all(&cache).unwrap();
    fs::write(&blocker, "no scratch here").unwrap();
    let result =
        instance_settings::rename(&other, &instance, "Changed", &EditCancellation::default());
    assert!(matches!(result, Err(EditError::Worker(_))));
    assert_eq!(fs::read(&marker).unwrap(), original);
    fs::remove_file(&blocker).unwrap();
    invalid_edits_leave_the_marker_alone(&other, &instance, &blocker, &marker, &original);
    instance_settings::rename(&other, &instance, "Changed", &EditCancellation::default()).unwrap();
    assert_eq!(name(&instance), "Changed");
    assert_eq!(name(baseline), "Fixture");
    assert!(!fixture.data.path(DataPath::Cache).join("workers").exists());
    let deadline = Instant::now() + DEADLINE;
    while fs::read_dir(&blocker).unwrap().count() != 0 {
        assert!(
            Instant::now() < deadline,
            "overridden cache was not cleaned up"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn name(instance: &InstanceTarget) -> String {
    profile::read(instance.directory())
        .unwrap()
        .unwrap()
        .instance
        .name
}

fn invalid_edits_leave_the_marker_alone(
    data: &DataDir,
    instance: &InstanceTarget,
    scratch: &Path,
    marker: &Path,
    original: &[u8],
) {
    let cancel = EditCancellation::default();
    cancel.cancel();
    assert!(instance_settings::rename(data, instance, "Cancelled", &cancel).is_err());
    assert!(!scratch.exists());
    let invalid_heap = instance_settings::set_instance_heap(
        data,
        instance,
        InstanceHeap::ManualMib(15),
        &EditCancellation::default(),
    );
    assert!(
        matches!(invalid_heap, Err(EditError::InvalidHeap)),
        "{invalid_heap:?}"
    );
    let invalid_name =
        instance_settings::rename(data, instance, "bad\nname", &EditCancellation::default());
    assert!(
        matches!(invalid_name, Err(EditError::InvalidName)),
        "{invalid_name:?}"
    );
    assert_eq!(fs::read(marker).unwrap(), original);
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn settings_worker_is_lazy_shared_while_busy_and_reaped_after_use() {
    let fixture = Fixture::new();
    let settings = Service::<Reply>::for_executable(
        fixture.images.instance_settings,
        fixture.executable.clone(),
    );
    assert!(settings.running_id().is_none());
    assert!(!fixture.has_extracted_libraries());
    let hold = settings.hold();
    let mut first = settings
        .request(&fixture.data, Edit::Rename(" First ".into()))
        .unwrap();
    let pid = settings.running_id().unwrap();
    assert!(fixture.has_extracted_libraries());
    let mut other = settings
        .request(&fixture.data, Edit::InstanceHeap(InstanceHeap::Inherit))
        .unwrap();
    assert_eq!(settings.running_id(), Some(pid));
    run(async {
        assert!(matches!(
            first.next().await,
            Some(Event::Reply(Reply::Validated(ValidatedEdit::Rename(name)))) if name == "First"
        ));
        assert!(matches!(
            other.next().await,
            Some(Event::Reply(Reply::Validated(ValidatedEdit::InstanceHeap(
                None
            ))))
        ));
        finish(&mut first).await.unwrap();
        finish(&mut other).await.unwrap();
    });
    drop(hold);
    fixture.wait_for_cleanup();
    assert!(settings.running_id().is_none());
    let mut retry = settings
        .request(&fixture.data, Edit::GlobalHeap(GlobalHeap::ManualMib(15)))
        .unwrap();
    run(async {
        assert!(matches!(
            retry.next().await,
            Some(Event::Reply(Reply::Rejected(InvalidEdit::InvalidHeap)))
        ));
        finish(&mut retry).await.unwrap();
    });
    fixture.wait_for_cleanup();
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn mismatched_settings_image_is_refused_without_a_write() {
    let fixture = Fixture::new();
    let wrong = Images::new(b"", b"", b"not the embedded settings image");
    let settings =
        Service::<Reply>::for_executable(wrong.instance_settings, fixture.executable.clone());
    let error = settings
        .request(&fixture.data, Edit::Rename("Not applied".into()))
        .err()
        .unwrap();
    assert!(error.contains("worker library does not match the embedded image"));
    assert!(
        !Document::Settings
            .path(fixture.data.path(DataPath::Root))
            .exists()
    );
    fixture.wait_for_cleanup();
}

fn startup_inspection_preview_and_offline_launch_do_not_start_settings(
    fixture: &Fixture,
    instance: &InstanceTarget,
) {
    let settings = state::load_settings(fixture.data.path(DataPath::Root)).unwrap();
    let game = fixture.data.game();
    let manifest = game.version_path("1.21.11", VersionPath::Document);
    fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    fs::write(
        &manifest,
        r#"{
            "id":"1.21.11",
            "mainClass":"Fixture",
            "javaVersion":{"majorVersion":42},
            "downloads":{"client":{"size":6,"url":"https://example.invalid/client.jar"}}
        }"#,
    )
    .unwrap();
    fs::write(
        game.version_path("1.21.11", VersionPath::ClientJar),
        b"client",
    )
    .unwrap();
    register_fixture_runtime(fixture);
    let profiles = profile::discover(fixture.data.path(DataPath::Profiles));
    let job = LaunchJob::for_profile(
        &fixture.data,
        &profiles.instances[0],
        "Player".into(),
        Some(&settings.data),
    )
    .unwrap();
    assert_eq!(job.instance_directory(), instance.directory());
    assert!(job.inspect().unwrap().java().is_some());
    let session = Session::offline("Player");
    assert!(job.preview(&session).is_ok());
    assert_launch_starts(job, session);
    assert!(!fixture.data.path(DataPath::Cache).join("workers").exists());
}

fn register_fixture_runtime(fixture: &Fixture) {
    let home = fixture.root.path().join("fixture runtime");
    let bin = home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let executable = bin.join(if cfg!(windows) { "java.exe" } else { "java" });
    fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    fs::write(home.join("release"), "JAVA_VERSION=\"42.0.1\"\n").unwrap();
    java::add(&fixture.data, &executable).unwrap();
}

fn assert_launch_starts(job: LaunchJob, session: Session) {
    let started = std::sync::Arc::new(AtomicBool::new(false));
    let process = started.clone();
    let outcome = job.run(
        Player::Offline(session),
        DownloadOptions::from_settings(None),
        |_| panic!("offline launch must not resolve an online account"),
        move || process.store(true, Ordering::Release),
    );
    assert!(started.load(Ordering::Acquire), "{outcome:?}");
    assert!(matches!(outcome, Err(LaunchFailure::Failed(_))));
}
