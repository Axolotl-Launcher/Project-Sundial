// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[path = "account_tests.rs"]
mod account_tests;
use crate::SystemJava;
use axlauncher_spi::LaunchCommand;
use std::fs;
use std::future::Future;
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::Poll;

const VERSION_ID: &str = "1.21";
const MAIN_CLASS: &str = "net.minecraft.client.main.Main";
const PLAYER_NAME: &str = "Player";

/// The native entries the fake jar carries, relative to the natives
/// directory; anything with another extension is ignored.
const NATIVE_FILES: [&str; 2] = ["example.dll", "nested/example.so"];

const IGNORED_ENTRY: &str = "META-INF/MANIFEST.MF";

const NATIVE_JAR_ENTRIES: [(&str, &[u8]); 3] = [
    (NATIVE_FILES[0], b"first native"),
    (NATIVE_FILES[1], b"second native"),
    (IGNORED_ENTRY, b"not a native"),
];

/// A version document with one library, whose natives the host always uses.
const VERSION_JSON: &str = r#"{
  "id": "1.21",
  "type": "release",
  "mainClass": "net.minecraft.client.main.Main",
  "libraries": [
    {
      "name": "org.example:example:1.0",
      "downloads": {
        "artifact": { "path": "org/example/example.jar" },
        "classifiers": {
          "natives-linux": { "path": "org/example/example-natives.jar" },
          "natives-windows": { "path": "org/example/example-natives.jar" },
          "natives-osx": { "path": "org/example/example-natives.jar" }
        }
      },
      "natives": {
        "linux": "natives-linux",
        "windows": "natives-windows",
        "osx": "natives-osx"
      }
    }
  ],
  "arguments": {
    "jvm": [
      "-Djava.library.path=${natives_directory}",
      "-cp",
      "${classpath}"
    ],
    "game": [
      "--username",
      "${auth_player_name}",
      "--uuid",
      "${auth_uuid}",
      "--accessToken",
      "${auth_access_token}",
      "--legacy-session=${auth_session}",
      "--userType",
      "${user_type}",
      "--version",
      "${version_name}",
      "--gameDir",
      "${game_directory}",
      "--assetIndex",
      "${assets_index_name}"
    ]
  },
  "assetIndex": { "id": "34" }
}"#;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A directory of its own per test, removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("axlauncher-flow-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).expect("temp directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A game root, an instance and a Java executable, all fake.
struct Fixture {
    temp: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            temp: TempDir::new(),
        };
        write_file(&fixture.version_document(), VERSION_JSON.as_bytes());
        write_file(&fixture.client_jar(), b"client jar");
        write_jar(&fixture.natives_jar(), &NATIVE_JAR_ENTRIES);
        write_file(&fixture.java(), b"fake java");
        fixture
    }

    fn game_root(&self) -> PathBuf {
        self.temp.path().join("meta")
    }

    fn instance_directory(&self) -> PathBuf {
        self.temp.path().join("profiles/instance")
    }

    fn java(&self) -> PathBuf {
        self.temp.path().join("bin").join("java")
    }

    fn natives_directory(&self) -> PathBuf {
        self.game_root().join("natives").join(VERSION_ID)
    }

    fn natives_jar(&self) -> PathBuf {
        self.game_root()
            .join("libraries")
            .join("org")
            .join("example")
            .join("example-natives.jar")
    }

    fn version_document(&self) -> PathBuf {
        self.game_root()
            .join(VERSIONS)
            .join(VERSION_ID)
            .join(format!("{VERSION_ID}.json"))
    }

    fn client_jar(&self) -> PathBuf {
        plan::client_jar(&self.game_root(), VERSION_ID)
    }

    fn request(&self) -> LaunchRequest {
        self.request_with_java(Some(self.java()))
    }

    fn request_with_java(&self, java_preference: Option<PathBuf>) -> LaunchRequest {
        LaunchRequest {
            game_root: self.game_root(),
            instance_directory: self.instance_directory(),
            version_id: VERSION_ID.to_owned(),
            java_preference,
            player_name: PLAYER_NAME.to_owned(),
        }
    }
}

fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directory");
    }
    fs::write(path, bytes).expect("fixture file");
}

fn write_jar(path: &Path, entries: &[(&str, &[u8])]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("jar directory");
    }
    let file = fs::File::create(path).expect("jar file");
    let mut writer = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, '_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

    for (name, bytes) in entries {
        writer.start_file(*name, options).expect("jar entry");
        writer.write_all(bytes).expect("jar bytes");
    }
}

/// A Java provider with a fixed answer that records the mode it was asked
/// for.
#[derive(Default)]
struct FakeJava {
    executable: Option<PathBuf>,
    modes: Mutex<Vec<JavaMode>>,
}

impl FakeJava {
    fn found(executable: PathBuf) -> Self {
        Self {
            executable: Some(executable),
            modes: Mutex::new(Vec::new()),
        }
    }

    fn missing() -> Self {
        Self::default()
    }

    fn modes(&self) -> Vec<JavaMode> {
        self.modes.lock().expect("recorded modes").clone()
    }
}

impl Java for FakeJava {
    fn executable(&self, _preferred: Option<&Path>, mode: JavaMode) -> Option<PathBuf> {
        self.modes.lock().expect("recorded modes").push(mode);
        self.executable.clone()
    }

    fn executable_for(&self, _major: u32, mode: JavaMode) -> Option<PathBuf> {
        self.executable(None, mode)
    }
}

/// A launcher that keeps every command it was given.
#[derive(Default)]
struct Recorder {
    commands: Mutex<Vec<LaunchCommand>>,
}

impl Recorder {
    fn commands(&self) -> Vec<LaunchCommand> {
        self.commands.lock().expect("recorded commands").clone()
    }
}

impl Launcher for Recorder {
    async fn launch(&self, command: LaunchCommand) -> Result<(), LaunchError> {
        self.commands
            .lock()
            .expect("recorded commands")
            .push(command);
        Ok(())
    }
}

/// A launcher that fails before anything starts.
struct Refusing;

impl Launcher for Refusing {
    async fn launch(&self, _command: LaunchCommand) -> Result<(), LaunchError> {
        Err(LaunchError::NotFound("no such executable".to_owned()))
    }
}

/// Runs `future` to completion on this thread, parking between polls.
fn finish<F: Future>(future: F) -> F::Output {
    let waker = crate::process::current_thread_waker();
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[test]
fn the_command_runs_the_main_class_with_every_placeholder_filled() {
    let fixture = Fixture::new();
    let java = FakeJava::found(fixture.java());

    let outcome = finish(launch(&java, &Recorder::default(), fixture.request()))
        .expect("a complete fixture launches");

    assert!(
        outcome
            .command
            .args
            .iter()
            .any(|argument| argument == MAIN_CLASS)
    );
    let joined = outcome.command.args.join(" ");
    assert!(!joined.contains("${"), "unresolved placeholder in {joined}");
    assert_eq!(
        outcome.command.program,
        fixture.java().display().to_string()
    );
    assert_eq!(
        outcome.command.working_directory,
        fixture.instance_directory()
    );
    assert_eq!(java.modes(), vec![JavaMode::Windowed]);
}

#[test]
fn preview_matches_the_launch_command_without_extracting_natives() {
    let fixture = Fixture::new();
    let java = FakeJava::found(fixture.java());
    let request = fixture.request();

    let command = preview(&java, &request).expect("preview");
    assert!(!fixture.natives_directory().exists());
    assert_eq!(command.working_directory, fixture.instance_directory());

    let outcome = finish(launch(&java, &Recorder::default(), request)).expect("launch");
    assert_eq!(command, outcome.command);
}

#[test]
fn inspection_keeps_the_declared_parent_and_reports_matching_java() {
    let fixture = Fixture::new();
    let mut request = fixture.request();
    request.version_id = "with-loader".into();
    write_file(
        &fixture
            .game_root()
            .join("versions/with-loader/with-loader.json"),
        br#"{"id":"with-loader","inheritsFrom":"1.21"}"#,
    );

    let inspection = inspect_version(&FakeJava::found(fixture.java()), &request).expect("inspect");
    assert_eq!(inspection.parent(), Some(VERSION_ID));
    assert_eq!(inspection.version().main_class, MAIN_CLASS);
    assert_eq!(inspection.java(), Some(fixture.java().as_path()));
    assert!(!fixture.natives_directory().exists());
}

#[test]
fn the_outcome_counts_the_natives_that_landed_on_disk() {
    let fixture = Fixture::new();
    let java = FakeJava::found(fixture.java());

    let outcome = finish(launch(&java, &Recorder::default(), fixture.request()))
        .expect("a complete fixture launches");

    assert_eq!(outcome.natives, NATIVE_FILES.len());
    for relative in NATIVE_FILES {
        let path = fixture.natives_directory().join(relative);
        assert!(path.is_file(), "missing {}", path.display());
    }
}

#[test]
fn a_missing_version_document_reports_the_path_it_tried() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.version_document()).expect("remove the document");
    let java = FakeJava::found(fixture.java());

    let error = finish(launch(&java, &Recorder::default(), fixture.request()))
        .expect_err("no version document");

    match &error {
        FlowError::VersionDocument {
            source: VersionError::Read { path, .. },
        } => assert_eq!(path, &fixture.version_document()),
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(
        error
            .to_string()
            .contains(&fixture.version_document().display().to_string())
    );
}

#[test]
fn a_missing_java_reports_no_java() {
    let fixture = Fixture::new();
    let java = FakeJava::missing();

    let error = finish(launch(
        &java,
        &Recorder::default(),
        fixture.request_with_java(None),
    ))
    .expect_err("no java");

    assert!(
        matches!(error, FlowError::NoJava { .. }),
        "unexpected: {error:?}"
    );
}

#[test]
fn a_missing_client_jar_reports_the_plan_error() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.client_jar()).expect("remove the client jar");
    let expected = fixture.client_jar();
    let java = FakeJava::found(fixture.java());

    let error =
        finish(launch(&java, &Recorder::default(), fixture.request())).expect_err("no client jar");

    match &error {
        FlowError::Plan {
            source: PlanError::NoClientJar { path },
        } => assert_eq!(path, &expected),
        other => panic!("unexpected error: {other:?}"),
    }
    let source = std::error::Error::source(&error).expect("the plan error is the source");
    assert!(
        source.downcast_ref::<PlanError>().is_some(),
        "the source should be the plan's own error"
    );
}

#[test]
fn the_launcher_receives_exactly_the_command_the_flow_built() {
    let fixture = Fixture::new();
    let launcher = Recorder::default();
    let java = FakeJava::found(fixture.java());

    let outcome = finish(launch(&java, &launcher, fixture.request())).expect("launch");

    assert_eq!(launcher.commands(), vec![outcome.command]);
}

#[test]
fn a_launcher_failure_reports_the_program_and_its_own_error() {
    let fixture = Fixture::new();
    let java = FakeJava::found(fixture.java());

    let error = finish(launch(&java, &Refusing, fixture.request())).expect_err("refused");

    match &error {
        FlowError::Launch { program, source } => {
            assert_eq!(program, &fixture.java());
            assert!(matches!(source, LaunchError::NotFound(_)));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn the_system_provider_resolves_the_preferred_executable() {
    let fixture = Fixture::new();
    let java = SystemJava::with_environment(None, None);

    let outcome = finish(launch(&java, &Recorder::default(), fixture.request()))
        .expect("the preferred java exists");

    assert_eq!(
        outcome.command.program,
        fixture.java().display().to_string()
    );
}

#[test]
fn the_system_provider_reports_no_java_without_one_to_use() {
    let fixture = Fixture::new();
    let java = SystemJava::with_environment(None, None);

    let error = finish(launch(
        &java,
        &Recorder::default(),
        fixture.request_with_java(None),
    ))
    .expect_err("nothing configured");

    assert!(
        matches!(error, FlowError::NoJava { .. }),
        "unexpected: {error:?}"
    );
}
