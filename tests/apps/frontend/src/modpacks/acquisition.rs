// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Write;

use axlauncher_core::spi::auth::{AuthNetwork, Request, Response, TransportError};
use axlauncher_core::state::profile::{StagedProfile, discover};
use axlauncher_modpack::{ModpackPlugin, PlanOptions};

use super::{ManualFile, Progress, supply_manual_files, test_zip as zip};

struct Offline;

impl AuthNetwork for Offline {
    async fn send(&self, _: Request<'_>) -> Result<Response, TransportError> {
        panic!("no metadata request is needed for this test plan")
    }
}

#[test]
fn successive_manual_attempts_keep_the_staged_files_and_check_the_pinned_digest() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("fixture.mrpack");
    let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
    zip.start_file(
        "modrinth.index.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(br#"{
      "formatVersion":1,"game":"minecraft","name":"Fixture","versionId":"1",
      "dependencies":{"minecraft":"1.20.1"},"files":[{"path":"mods/pending.jar",
      "fileSize":3,"downloads":["https://cdn.modrinth.com/data/fixture.jar"],
      "hashes":{"sha1":"a9993e364706816aba3e25717850c26c9cd0d89d",
      "sha512":"ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"}}]}
    "#).unwrap();
    zip.finish().unwrap();
    let pack = ModpackPlugin::builtin().inspect(&archive).unwrap();
    let plan =
        futures::executor::block_on(pack.plan(&BTreeSet::new(), PlanOptions::default(), &Offline))
            .unwrap();
    drop(pack);
    let instances = temp.path().join("profiles");
    let staging = StagedProfile::new(&instances).unwrap();
    let kept = staging.directory().join("already-downloaded.jar");
    fs::write(&kept, b"keep").unwrap();
    let input = temp.path().join("supplied.jar");
    let local = BTreeMap::from([(String::from("mods/pending.jar"), input.clone())]);
    let (progress, _) = tokio::sync::watch::channel(Progress::Planning);
    let pending = vec![ManualFile {
        path: "mods/pending.jar".into(),
        url: String::new(),
        reason: None,
    }];

    fs::write(&input, b"bad").unwrap();
    let pending = supply_manual_files(pending, &plan, staging.directory(), &local, &progress);
    assert_eq!(pending[0].reason(), Some("SHA-1 mismatch"));
    assert!(!staging.directory().join("mods/pending.jar").exists());

    fs::write(&input, b"abc").unwrap();
    assert!(supply_manual_files(pending, &plan, staging.directory(), &local, &progress).is_empty());
    assert_eq!(
        fs::read(staging.directory().join("mods/pending.jar")).unwrap(),
        b"abc"
    );
    assert_eq!(fs::read(kept).unwrap(), b"keep");
    assert!(discover(&instances).instances.is_empty());
}
