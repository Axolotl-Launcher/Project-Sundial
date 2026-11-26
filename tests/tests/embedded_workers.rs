//! Exercises the actual desktop executable, copied without its build tree.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use axlauncher_spi::downloads::DownloadJob;
use axlauncher_storage::{DataDir, Directory};
use axlauncher_worker::accounts::{Action, Change, Command, Method, Reply};
use axlauncher_worker::downloads::{Route, Transfer};
use axlauncher_worker::{Event, Images, Request, Service};

const DEADLINE: Duration = Duration::from_secs(15);

struct Fixture {
    root: tempfile::TempDir,
    executable: PathBuf,
    data: DataDir,
    images: Images,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let executable = PathBuf::from(std::env::var_os("AXLAUNCHER_SMOKE_EXE").unwrap());
        let copied = root.path().join(executable.file_name().unwrap());
        fs::copy(executable, &copied).unwrap();
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        let mut data = DataDir::isolated(root.path().join("profile root 雪")).unwrap();
        let temp = root.path().join("unavailable temp directory");
        data.set_directory(Directory::AppTemp, Some(&temp)).unwrap();
        fs::write(temp, b"not a directory").unwrap();
        Self {
            root,
            executable: copied,
            data,
            images: Images::new(
                image("AXLAUNCHER_DOWNLOAD_IMAGE"),
                image("AXLAUNCHER_ACCOUNT_IMAGE"),
            ),
        }
    }

    fn accounts(&self) -> Service<Reply> {
        Service::for_executable(self.images.accounts, self.executable.clone())
    }

    fn downloads(&self) -> Service<axlauncher_worker::downloads::Reply> {
        Service::for_executable(self.images.downloads, self.executable.clone())
    }

    fn account(&self, worker: &Service<Reply>, action: Action) -> Request<Reply> {
        worker
            .request(
                &self.data,
                Command::new(self.data.root().to_path_buf(), action),
            )
            .unwrap()
    }

    fn wait_for_cleanup(&self) {
        let deadline = Instant::now() + DEADLINE;
        while self.has_extracted_libraries() {
            assert!(
                Instant::now() < deadline,
                "an extracted library was not cleaned up"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn has_extracted_libraries(&self) -> bool {
        match fs::read_dir(self.data.caches().join("workers")) {
            Ok(mut entries) => entries.next().is_some(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => panic!("could not inspect worker scratch: {error}"),
        }
    }
}

fn image(variable: &str) -> &'static [u8] {
    fs::read(std::env::var_os(variable).unwrap())
        .unwrap()
        .leak()
}

fn run<Output>(future: impl Future<Output = Output>) -> Output {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async { tokio::time::timeout(DEADLINE, future).await.unwrap() })
}

async fn finish<Reply>(request: &mut Request<Reply>) -> Result<(), String> {
    loop {
        match request.next().await.unwrap() {
            Event::Finished(result) => return result,
            Event::Reply(_) => (),
        }
    }
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn one_executable_loads_accounts_only_on_demand() {
    let fixture = Fixture::new();
    let worker = fixture.accounts();
    assert!(!fixture.has_extracted_libraries());
    let hold = worker.hold();
    let mut add = fixture.account(
        &worker,
        Action::Change(Change::AddOffline {
            name: "Notch".into(),
            uuid: String::new(),
        }),
    );
    let pid = worker.running_id().unwrap();
    run(finish(&mut add)).unwrap();
    let mut read = fixture.account(&worker, Action::Change(Change::Read));
    assert_eq!(worker.running_id(), Some(pid));
    run(assert_account(&mut read));
    drop(hold);
    fixture.wait_for_cleanup();
    assert!(worker.running_id().is_none());

    let mut read = fixture.account(&worker, Action::Change(Change::Read));
    run(assert_account(&mut read));
    fixture.wait_for_cleanup();
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn account_worker_never_reuses_sessions_across_data_roots() {
    let fixture = Fixture::new();
    let worker = fixture.accounts();
    let hold = worker.hold();
    let mut add = fixture.account(
        &worker,
        Action::Change(Change::AddOffline {
            name: "Notch".into(),
            uuid: String::new(),
        }),
    );
    run(assert_account(&mut add));

    let other = DataDir::isolated(fixture.root.path().join("other profile")).unwrap();
    let action = || Command::new(other.root().to_path_buf(), Action::Change(Change::Read));
    let mut wrong_root = worker.request(&other, action()).unwrap();
    assert!(
        run(finish(&mut wrong_root))
            .unwrap_err()
            .contains("different data directory")
    );
    drop(hold);
    worker.close_and_wait();
    fixture.wait_for_cleanup();

    let worker = fixture.accounts();
    let mut read = worker.request(&other, action()).unwrap();
    run(async {
        let Some(Event::Reply(Reply::Snapshot { snapshot, .. })) = read.next().await else {
            panic!("expected a snapshot of the new data root");
        };
        assert!(snapshot.selected().is_none());
        finish(&mut read).await.unwrap();
    });
    worker.close_and_wait();
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn mismatched_worker_image_is_refused_and_removed() {
    let fixture = Fixture::new();
    let wrong = Images::new(b"not the embedded library", b"not the embedded library");
    let worker = Service::<Reply>::for_executable(wrong.accounts, fixture.executable.clone());
    let request = worker.request(
        &fixture.data,
        Command::new(
            fixture.data.root().to_path_buf(),
            Action::Change(Change::Read),
        ),
    );
    let error = request.err().unwrap();
    assert!(error.contains("worker library does not match the embedded image"));
    assert!(error.contains("restart the launcher"));
    assert!(!fixture.has_extracted_libraries());
    assert!(!fixture.data.accounts().exists());
}

async fn assert_account(read: &mut Request<Reply>) {
    let Some(Event::Reply(Reply::Snapshot { snapshot, notice })) = read.next().await else {
        panic!("expected account snapshot");
    };
    assert!(notice.is_none());
    assert_eq!(snapshot.iter().len(), 1);
    assert_eq!(snapshot.selected().unwrap().name(), "Notch");
    finish(read).await.unwrap();
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn cancelling_browser_sign_in_releases_the_callback_and_account_worker() {
    let fixture = Fixture::new();
    let worker = fixture.accounts();
    let action = || Action::Microsoft {
        method: Method::Browser,
        expected: None,
        proxy: None,
    };
    for _ in 0..2 {
        let mut login = fixture.account(&worker, action());
        run(async {
            assert!(matches!(
                login.next().await,
                Some(Event::Reply(Reply::Browser { .. }))
            ));
            login.cancel();
            assert!(matches!(
                login.next().await,
                Some(Event::Reply(Reply::Cancelled))
            ));
            finish(&mut login).await.unwrap();
        });
        fixture.wait_for_cleanup();
    }
    assert!(!fixture.data.accounts().exists());
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn downloads_share_a_worker_and_cancel_without_publishing() {
    let fixture = Fixture::new();
    let worker = fixture.downloads();
    fs::create_dir_all(fixture.data.root()).unwrap();
    let destination = fixture.data.root().join("complete.bin");
    let cancelled = fixture.data.root().join("cancelled.bin");
    fs::write(&cancelled, b"previous").unwrap();
    let (url, server) = local_file(false);
    let (slow, stalled) = local_file(true);
    let hold = worker.hold();
    let mut first = worker
        .request(&fixture.data, transfer(&url, &destination))
        .unwrap();
    let pid = worker.running_id().unwrap();
    let mut pending = worker
        .request(&fixture.data, transfer(&slow, &cancelled))
        .unwrap();
    assert_eq!(worker.running_id(), Some(pid));
    stalled.ready.recv_timeout(DEADLINE).unwrap();
    pending.cancel();
    assert!(run(finish(&mut pending)).unwrap_err().contains("cancel"));
    run(finish(&mut first)).unwrap();
    assert_eq!(fs::read(destination).unwrap(), b"abc");
    assert_eq!(fs::read(cancelled).unwrap(), b"previous");
    assert_eq!(fs::read_dir(fixture.data.root()).unwrap().count(), 3);
    drop(hold);
    drop((server, stalled));
    fixture.wait_for_cleanup();
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn crashed_worker_fails_the_request_and_the_next_request_starts_a_new_worker() {
    let fixture = Fixture::new();
    let worker = fixture.downloads();
    let destination = fixture.data.root().join("recovered.bin");
    let (url, stalled) = local_file(true);
    let mut first = worker
        .request(&fixture.data, transfer(&url, &destination))
        .unwrap();
    stalled.ready.recv_timeout(DEADLINE).unwrap();
    kill(worker.running_id().unwrap());
    assert!(
        run(finish(&mut first))
            .unwrap_err()
            .contains("worker exited")
    );
    fixture.wait_for_cleanup();
    assert!(!destination.exists());
    drop(stalled);

    let (url, _server) = local_file(false);
    let mut next = worker
        .request(&fixture.data, transfer(&url, &destination))
        .unwrap();
    run(finish(&mut next)).unwrap();
    assert_eq!(fs::read(destination).unwrap(), b"abc");
    fixture.wait_for_cleanup();
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn worker_shutdown_reaps_a_live_download_before_removing_its_library() {
    let fixture = Fixture::new();
    let worker = fixture.downloads();
    let destination = fixture.data.root().join("interrupted.bin");
    let (url, stalled) = local_file(true);
    let mut request = worker
        .request(&fixture.data, transfer(&url, &destination))
        .unwrap();
    stalled.ready.recv_timeout(DEADLINE).unwrap();
    assert!(fixture.has_extracted_libraries());

    worker.close_and_wait();

    assert!(!fixture.has_extracted_libraries());
    assert!(!destination.exists());
    assert!(run(finish(&mut request)).is_err());
}

fn kill(pid: u32) {
    let pid = pid.to_string();
    let mut command;
    #[cfg(windows)]
    {
        command = std::process::Command::new("taskkill");
        command.args(["/F", "/PID", &pid]);
    }
    #[cfg(unix)]
    {
        command = std::process::Command::new("kill");
        command.args(["-KILL", &pid]);
    }
    assert!(command.status().unwrap().success());
}

fn transfer(url: &str, destination: &Path) -> Transfer {
    Transfer::new(
        Route::default(),
        DownloadJob {
            sources: &[url],
            destination,
            sha1: Some("a9993e364706816aba3e25717850c26c9cd0d89d"),
            sha1_alternatives: &[],
            size: Some(3),
            batch: true,
        },
        None,
    )
    .unwrap()
}

struct LocalFile {
    ready: std::sync::mpsc::Receiver<()>,
    release: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for LocalFile {
    fn drop(&mut self) {
        drop(self.release.take());
        if let Some(thread) = self.thread.take() {
            let result = thread.join();
            assert!(
                result.is_ok() || std::thread::panicking(),
                "HTTP fixture failed"
            );
        }
    }
}

fn local_file(stall: bool) -> (String, LocalFile) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let (sender, ready) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let mut stream = accept(&listener);
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(DEADLINE)).unwrap();
        let mut request = BufReader::new(&stream).lines().map(Result::unwrap);
        assert_eq!(request.next().as_deref(), Some("GET /file HTTP/1.1"));
        assert!(request.any(|line| line.is_empty()));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\n")
            .unwrap();
        let _ = sender.send(());
        if stall {
            let _ = released.recv_timeout(DEADLINE);
        } else {
            stream.write_all(b"abc").unwrap();
        }
    });
    (
        url,
        LocalFile {
            ready,
            release: Some(release),
            thread: Some(thread),
        },
    )
}

fn accept(listener: &TcpListener) -> std::net::TcpStream {
    let deadline = Instant::now() + DEADLINE;
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(error) => panic!("accepting download request: {error}"),
        }
    }
}
