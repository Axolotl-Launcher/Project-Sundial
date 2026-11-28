use std::fs;
use std::io::{Cursor, Write};
use std::process::Command;
use std::time::Duration;

use axlauncher_install::loader::check_files;
use axlauncher_launch::Target;
use axlauncher_network_spi::{FetchError, Network};
use axlauncher_spi::credentials::CURSEFORGE_API_KEY;
use axlauncher_spi::{
    Java, JavaMode, LoaderError, LoaderFile, LoaderKind, LoaderRequest, LoaderVersion, ModLoader,
};
use axlauncher_storage::GameAccess;

use super::*;

#[path = "../../tests/stand_in.rs"]
mod stand_in;

const VANILLA: &str = "local-vanilla";
const LOADER: &str = "local-processor-loader";
const TOOL: &str = "fixture:binarypatcher:1";
const CLIENT: &[u8] = b"abc";
const PROCESSOR_ENV_PROBE: &str = "AXLAUNCHER_TEST_PROCESSOR_ENV";

struct Fixture {
    _scratch: tempfile::TempDir,
    game: GameDir,
    java: PathBuf,
    profile: LoaderProfile,
}

impl Fixture {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let game = GameDir::standalone(scratch.path().join("game root 雪"));
        let path = game.version_path(VANILLA, VersionPath::Document);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            serde_json::to_vec(&serde_json::json!({
                "id": VANILLA,
                "downloads": {"client": {"url": "fixture://client", "size": CLIENT.len()}}
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(game.version_path(VANILLA, VersionPath::ClientJar), CLIENT).unwrap();
        let java = stand_in::compile(scratch.path());
        let profile = LoaderProfile {
            version_id: LOADER.into(),
            document: serde_json::to_vec(&serde_json::json!({
                "id": LOADER,
                "inheritsFrom": VANILLA,
                "processors": [{"jar": TOOL, "args": ["--output", "{ROOT}/patched.jar"]}]
            }))
            .unwrap(),
            files: vec![LoaderFile {
                path: "libraries/fixture/binarypatcher/1/binarypatcher-1.jar".into(),
                bytes: jar(),
            }],
        };
        Self {
            _scratch: scratch,
            game,
            java,
            profile,
        }
    }

    fn mode(&self, mode: &str) {
        fs::write(self.game.root().join("processor.mode"), mode).unwrap();
    }

    fn output(&self) -> Vec<u8> {
        fs::read(self.game.root().join("patched.jar")).unwrap()
    }

    async fn install(&self, cancel: &Cancellation) -> Result<LoaderInstalled, LoaderInstallError> {
        let _lock = self.game.lock(GameAccess::Install).unwrap();
        InstallRequest {
            network: &Offline,
            downloader: &axlauncher_download_engine::Direct(&Offline),
            loader: self,
            java: self,
            game_root: &self.game,
            request: LoaderRequest {
                game_version: VANILLA.into(),
                loader_version: None,
            },
            target: &Target::host(),
            concurrency: 1,
        }
        .install_cancellable(cancel, |_| {})
        .await
    }
}

fn jar() -> Vec<u8> {
    let mut jar = zip::ZipWriter::new(Cursor::new(Vec::new()));
    jar.start_file(
        "META-INF/MANIFEST.MF",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    jar.write_all(b"Main-Class: Fixture\r\n").unwrap();
    jar.finish().unwrap().into_inner()
}

impl Java for Fixture {
    fn executable(&self, _: JavaMode) -> Option<PathBuf> {
        Some(self.java.clone())
    }
    fn executable_for(&self, major: u32, _: JavaMode) -> Option<PathBuf> {
        (major == 8).then(|| self.java.clone())
    }

    fn executable_at(&self, path: &Path, major: Option<u32>, _: JavaMode) -> Option<PathBuf> {
        (path == self.java && major.is_none_or(|major| major == 8)).then(|| self.java.clone())
    }
}

impl ModLoader for Fixture {
    fn kind(&self) -> LoaderKind {
        LoaderKind::NeoForge
    }

    async fn versions(&self, _: &str) -> Result<Vec<LoaderVersion>, LoaderError> {
        unreachable!()
    }
    async fn profile(&self, _: LoaderRequest) -> Result<LoaderProfile, LoaderError> {
        Ok(self.profile.clone())
    }
}

struct Offline;

impl Network for Offline {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        panic!("unexpected fixture network request: {url}");
    }
}

fn run<F: Future>(test: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(20), test)
                .await
                .unwrap()
        })
}

#[test]
fn processors_do_not_inherit_the_curseforge_key() {
    if std::env::var_os(PROCESSOR_ENV_PROBE).is_some() {
        assert!(std::env::var_os(CURSEFORGE_API_KEY).is_some());
        let fixture = Fixture::new();
        fixture.mode("check-env");
        run(fixture.install(&Cancellation::default())).unwrap();
        assert_eq!(fixture.output(), b"complete");
        return;
    }

    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "loader::lifecycle_tests::processors_do_not_inherit_the_curseforge_key",
        ])
        .env(PROCESSOR_ENV_PROBE, "1")
        .env(CURSEFORGE_API_KEY, "synthetic-environment-probe")
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && report.contains("1 passed"),
        "{report}"
    );
}

#[test]
fn interrupted_processor_output_is_not_reused_even_when_it_exists() {
    let fixture = Fixture::new();
    fixture.mode("fail");
    let error = run(fixture.install(&Cancellation::default())).unwrap_err();
    assert!(error.to_string().contains("exited with 27"));
    assert_eq!(fixture.output(), b"partial!");
    let marker = ProcessorMarker::for_version(&fixture.game, LOADER);
    assert!(marker.is_incomplete().unwrap());
    assert!(
        check_files(&fixture.game, LOADER, &Target::host())
            .unwrap_err()
            .contains("did not finish")
    );
    assert!(stand_in::wait_dead(stand_in::wait_pid(fixture.game.root())));
    fixture.mode("pass");
    assert_eq!(
        run(fixture.install(&Cancellation::default()))
            .unwrap()
            .version_id,
        LOADER
    );
    assert_eq!(fixture.output(), b"complete");
    assert!(!marker.is_incomplete().unwrap());
    check_files(&fixture.game, LOADER, &Target::host()).unwrap();
}

#[test]
fn cancelling_waits_for_java_before_returning_and_keeps_the_incomplete_marker() {
    let fixture = Fixture::new();
    fixture.mode("hang");
    let cancel = Cancellation::default();
    let (result, pid) = run(async {
        tokio::join!(fixture.install(&cancel), async {
            let root = fixture.game.root().to_owned();
            let pid = tokio::task::spawn_blocking(move || stand_in::wait_pid(&root))
                .await
                .unwrap();
            assert!(fixture.game.lock(GameAccess::Launch).is_err());
            cancel.cancel();
            pid
        })
    });
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert!(stand_in::wait_dead(pid));
    assert!(
        ProcessorMarker::for_version(&fixture.game, LOADER)
            .is_incomplete()
            .unwrap()
    );
    assert!(fixture.game.lock(GameAccess::Launch).is_ok());
}
