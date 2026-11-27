use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use axlauncher_download_spi::{DownloadJob, FileDownloader};
use axlauncher_install::prepare::Progress;
use axlauncher_network::MirrorSettings;
use axlauncher_state::profile::{Game, Profile, StagedProfile};
use axlauncher_storage::{DataDir, DataPath, Directory, GameAccess, VersionPath};
use axlauncher_worker::downloads::{Downloads, Route};
use axlauncher_worker::installation::{Command, JavaPaths, Reply, Target, install};
use axlauncher_worker::{Cancellation, Event, Service};

use super::{DEADLINE, Fixture, LocalFile, finish, kill, local_file, run, stand_in};

const MINECRAFT: &str = "fixture-vanilla";
const CLIENT_HASH: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

fn manifest(data: &DataDir, url: &str) {
    let path = data.game().version_path(MINECRAFT, VersionPath::Document);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        serde_json::to_vec(&serde_json::json!({
            "id": MINECRAFT, "downloads": {"client": {"url": url, "size": 3, "sha1": CLIENT_HASH}}
        }))
        .unwrap(),
    )
    .unwrap();
}

fn command(data: &DataDir) -> Command {
    Command::new(
        data,
        Target::new(MINECRAFT.into(), None),
        MirrorSettings::default(),
        1,
        JavaPaths::new(Vec::new(), Vec::new()),
    )
}

async fn wait_http(server: &LocalFile) {
    let start = Instant::now();
    while server.ready.try_recv().is_err() {
        assert!(start.elapsed() < DEADLINE, "local request did not arrive");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn embedded_install_relays_files_and_drains_only_its_own_writes() {
    let mut fixture = Fixture::new();
    let cache = fixture
        .data
        .path(DataPath::Root)
        .with_file_name("alternate cache");
    fixture
        .data
        .set_directory(Directory::AppCache, Some(&cache))
        .unwrap();
    fixture
        .images
        .configure_for_executable(fixture.executable.clone());
    assert!(!fixture.has_extracted_libraries());
    let launch = fixture.data.game().lock(GameAccess::Launch).unwrap();
    let downloads = Downloads::new(fixture.data.clone(), Route::default());
    let error = run(install(
        &fixture.data,
        command(&fixture.data),
        downloads.clone(),
        Cancellation::default(),
        |_, _| unreachable!(),
        |_| unreachable!(),
    ))
    .unwrap_err();
    assert!(error.contains("Cannot lock shared game files"), "{error}");
    assert!(!fixture.has_extracted_libraries());
    drop(launch);

    relay_and_cancel(&fixture, &downloads);
    finish_staged(&fixture, downloads);
}

fn relay_and_cancel(fixture: &Fixture, downloads: &Downloads) {
    let game = fixture.data.game();
    let (url, stalled) = local_file(true);
    let (other_url, mut other_server) = local_file(true);
    manifest(&fixture.data, &url);
    let other_path = fixture.root.path().join("other.bin");
    let cancel = Cancellation::default();
    let data = &fixture.data;
    let (result, other) = run(async {
        let installation = install(
            data,
            command(data),
            downloads.clone(),
            cancel.clone(),
            |_, _| unreachable!(),
            |_| {},
        );
        let sources = [&*other_url];
        let other = downloads.download(download_job(&sources, &other_path), None);
        let mut installation = std::pin::pin!(installation);
        let mut other = std::pin::pin!(other);
        tokio::select! {
            result = &mut installation => panic!("installation completed before its stalled download: {result:?}"),
            result = &mut other => panic!("unrelated download did not stall at the fixture: {result:?}"),
            () = wait_for_relays(data, &stalled, &other_server) => (),
        }
        cancel.cancel();
        other_server.finish();
        tokio::join!(installation, other)
    });
    assert!(result.unwrap_err().contains("cancelled"));
    other.unwrap();
    assert_eq!(fs::read(other_path).unwrap(), b"abc");
    assert!(
        !game
            .version_path(MINECRAFT, VersionPath::ClientJar)
            .exists()
    );
    assert!(game.lock(GameAccess::Launch).is_ok());
    fixture.wait_for_cleanup();
}

async fn wait_for_relays(data: &DataDir, stalled: &LocalFile, other: &LocalFile) {
    wait_http(stalled).await;
    wait_http(other).await;
    assert_eq!(
        fs::read_dir(data.path(DataPath::Cache).join("workers"))
            .unwrap()
            .count(),
        2
    );
    assert!(data.game().lock(GameAccess::Launch).is_err());
    assert!(data.game().lock(GameAccess::Install).is_err());
}

fn download_job<'a>(sources: &'a [&'a str], destination: &'a Path) -> DownloadJob<'a> {
    DownloadJob {
        sources,
        destination,
        sha1: Some(CLIENT_HASH),
        sha1_alternatives: &[],
        size: Some(3),
        batch: true,
    }
}

fn finish_staged(fixture: &Fixture, downloads: Downloads) {
    let data = &fixture.data;
    let (url, _server) = local_file(false);
    manifest(data, &url);
    let stage = StagedProfile::new(data.path(DataPath::Profiles)).unwrap();
    let profile = Profile::new("install fixture", Some(Game::new(MINECRAFT)));
    let mut phases = Vec::new();
    let resolved = run(install(
        data,
        command(data),
        downloads,
        Cancellation::default(),
        |_, _| unreachable!(),
        |progress| phases.push(progress),
    ))
    .unwrap();
    assert!(phases.contains(&Progress::Downloading {
        processed: 1,
        total: 1
    }));
    assert_eq!(resolved, MINECRAFT);
    assert!(
        axlauncher_state::profile::discover(data.path(DataPath::Profiles))
            .instances
            .is_empty()
    );
    fs::write(stage.directory().join("pack-file"), b"pack files finished").unwrap();
    let published = stage.publish_installed(profile, &resolved).unwrap();
    assert!(published.join("pack-file").is_file());
    assert_eq!(
        fs::read(data.game().version_path(MINECRAFT, VersionPath::ClientJar)).unwrap(),
        b"abc"
    );
    fixture.wait_for_cleanup();
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn embedded_install_process_is_reaped_after_cancelled_request() {
    let fixture = Fixture::new();
    manifest(&fixture.data, "http://127.0.0.1:9/file");
    let worker =
        Service::<Reply>::for_executable(fixture.images.install, fixture.executable.clone());
    let mut request = worker
        .request(&fixture.data, command(&fixture.data))
        .unwrap();
    let pid = worker.running_id().unwrap();
    run(async {
        loop {
            match request.next().await {
                Some(Event::Reply(Reply::Download { .. })) => break,
                Some(Event::Reply(_)) => (),
                _ => panic!("worker stopped before it relayed a file"),
            }
        }
        request.cancel();
        assert!(
            finish(&mut request)
                .await
                .unwrap_err()
                .contains("cancelled")
        );
    });
    worker.close_and_wait();
    assert!(stand_in::wait_dead(pid));
    fixture.wait_for_cleanup();
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn killing_an_install_worker_terminates_its_child_writer() {
    let fixture = Fixture::new();
    let child = stand_in::compile(fixture.root.path());
    let worker = Service::<Reply>::for_executable(fixture.images.install, child);
    let mut request = worker
        .request(&fixture.data, command(&fixture.data))
        .unwrap();
    let worker_pid = worker.running_id().unwrap();
    let child_pid = stand_in::wait_pid(fixture.root.path());
    kill(worker_pid);
    assert!(run(finish(&mut request)).is_err());
    worker.close_and_wait();
    assert!(stand_in::wait_dead(worker_pid));
    assert!(stand_in::wait_dead(child_pid));
    fixture.wait_for_cleanup();
}
