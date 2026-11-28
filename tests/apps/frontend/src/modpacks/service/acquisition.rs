// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use axlauncher_core::network_spi::auth::{AuthNetwork, Request, Response, TransportError};
use axlauncher_core::network_spi::{FetchError, Network};
use axlauncher_core::state::profile::{Game, ProfileFolderName, StagedProfile, discover};
use axlauncher_core::storage::{DataDir, DataPath};
use axlauncher_modpack::{InstallPlan, ModpackPlugin, PlanOptions};

use super::{ManualFile, StagedPack, download_remaining, supply_manual_files, test_zip as zip};

struct Offline;

impl AuthNetwork for Offline {
    async fn send(&self, _: Request<'_>) -> Result<Response, TransportError> {
        panic!("no metadata request is needed for this test plan")
    }
}

fn fixture(root: &Path, files: &[&str]) -> InstallPlan {
    let archive = root.join("fixture.mrpack");
    let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
    zip.start_file(
        "modrinth.index.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    let files = files.iter().map(|file| format!(r#"{{"path":"mods/{file}",
        "fileSize":3,"downloads":["https://cdn.modrinth.com/data/{file}"],
        "hashes":{{"sha1":"a9993e364706816aba3e25717850c26c9cd0d89d",
        "sha512":"ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"}}}}"#)).collect::<Vec<_>>().join(",");
    write!(
        zip,
        r#"{{"formatVersion":1,"game":"minecraft","name":"Fixture","versionId":"1",
        "dependencies":{{"minecraft":"1.20.1"}},"files":[{files}]}}"#
    )
    .unwrap();
    zip.finish().unwrap();
    let pack = ModpackPlugin::builtin().inspect(&archive).unwrap();
    futures::executor::block_on(pack.plan(&BTreeSet::new(), PlanOptions::default(), &Offline))
        .unwrap()
}

#[test]
fn successive_manual_attempts_keep_the_staged_files_and_check_the_pinned_digest() {
    let temp = tempfile::tempdir().unwrap();
    let plan = fixture(temp.path(), &["pending.jar"]);
    let instances = temp.path().join("profiles");
    let staging = StagedProfile::new(&instances).unwrap();
    let kept = staging.directory().join("already-downloaded.jar");
    fs::write(&kept, b"keep").unwrap();
    let input = temp.path().join("supplied.jar");
    let local = BTreeMap::from([(String::from("mods/pending.jar"), input.clone())]);
    let pending = vec![ManualFile {
        path: "mods/pending.jar".into(),
        url: String::new(),
        reason: None,
    }];

    fs::write(&input, b"bad").unwrap();
    let pending = supply_manual_files(pending, &plan, staging.directory(), &local);
    assert_eq!(pending[0].reason(), Some("SHA-1 mismatch"));
    assert!(!staging.directory().join("mods/pending.jar").exists());

    fs::write(&input, b"abc").unwrap();
    assert!(supply_manual_files(pending, &plan, staging.directory(), &local).is_empty());
    assert_eq!(
        fs::read(staging.directory().join("mods/pending.jar")).unwrap(),
        b"abc"
    );
    assert_eq!(fs::read(kept).unwrap(), b"keep");
    assert!(discover(&instances).instances.is_empty());
}

#[test]
fn publishing_to_a_new_folder_keeps_the_pack_name_and_preserves_conflicting_directories() {
    let root = tempfile::tempdir().unwrap();
    let plan = fixture(root.path(), &[]);
    let data = DataDir::isolated(root.path()).unwrap();
    let instances = data.path(DataPath::Profiles);
    let pack = StagedPack::new(plan, &data).unwrap();
    let collision = instances.join("Fixture");
    fs::create_dir(&collision).unwrap();
    let folder = ProfileFolderName::new("Fixture").unwrap();
    let game = Game::new("1.20.1");
    fs::write(pack.staging.directory().join("prepared"), b"pack content").unwrap();

    assert!(folder.check_available(instances).is_err());
    assert!(pack.publish(&game, "1.20.1", &folder).is_err());
    assert_eq!(fs::read_dir(&collision).unwrap().count(), 0);
    assert!(discover(instances).instances.is_empty());

    let folder = ProfileFolderName::new("My modpack save").unwrap();
    let directory = pack.publish(&game, "1.20.1", &folder).unwrap();
    assert_eq!(directory, instances.join("My modpack save"));
    assert_eq!(
        fs::read(directory.join("prepared")).unwrap(),
        b"pack content"
    );
    let profiles = discover(instances);
    assert_eq!(profiles.instances.len(), 1);
    assert_eq!(profiles.instances[0].profile.instance.name, "Fixture");
}

#[test]
fn modpack_folders_must_be_portable_and_outside_launcher_namespaces() {
    for name in [
        "",
        "   ",
        "../Fixture",
        "a/b",
        "NUL.cfg",
        "CONIN$",
        "Pack.",
        ".Profile-Locks",
        ".STAGING-pack",
    ] {
        assert!(ProfileFolderName::new(name).is_err(), "{name:?}");
    }
    assert!(ProfileFolderName::new(&"a".repeat(256)).is_err());
    assert!(ProfileFolderName::new("整合包 1.0").is_ok());
}

struct FileServer<'a> {
    unreachable: Option<&'a str>,
    calls: RefCell<Vec<String>>,
}

impl Network for FileServer<'_> {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let file = url.rsplit('/').next().unwrap();
        self.calls.borrow_mut().push(file.into());
        if self.unreachable == Some(file) {
            return Err(FetchError::Fatal("file unavailable".into()));
        }
        Ok(b"abc".to_vec())
    }
}

#[test]
fn retry_only_requests_failed_pack_downloads() {
    let temp = tempfile::tempdir().unwrap();
    let plan = fixture(temp.path(), &["first.jar", "second.jar"]);
    let data = DataDir::isolated(temp.path()).unwrap();
    let mut pack = StagedPack::new(plan, &data).unwrap();
    let first = FileServer {
        unreachable: Some("first.jar"),
        calls: RefCell::default(),
    };
    let result = futures::executor::block_on(download_remaining(
        &mut pack,
        &axlauncher_download_engine::Direct(&first),
        1,
        &|_| {},
    ));
    assert!(result.unwrap_err().contains("mods/first.jar"));
    assert_eq!(
        fs::read(pack.staging.directory().join("mods/second.jar")).unwrap(),
        b"abc"
    );

    let retry = FileServer {
        unreachable: None,
        calls: RefCell::default(),
    };
    futures::executor::block_on(download_remaining(
        &mut pack,
        &axlauncher_download_engine::Direct(&retry),
        1,
        &|_| {},
    ))
    .unwrap();
    assert_eq!(retry.calls.into_inner(), ["first.jar"]);
    assert!(pack.remaining.is_empty());
    assert!(discover(data.path(DataPath::Profiles)).instances.is_empty());
}
