// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Write;
use std::path::Path;

use super::test_sha2::{Digest, Sha512};
use axlauncher_spi::auth::{AuthNetwork, Request, Response, TransportError};
use serde_json::{Value, json};
use zip::{ZipWriter, write::SimpleFileOptions};

use super::test_futures::executor::block_on;
use super::test_tempfile as tempfile;
use super::{InstallPlan, ModpackPlugin, OptionId, Pack, PackError, PackSource, PlanOptions};

mod multimc;

struct Offline;

impl AuthNetwork for Offline {
    async fn send(&self, _: Request<'_>) -> Result<Response, TransportError> {
        panic!("the local plan does not need authenticated metadata")
    }
}

fn local_plan(pack: &Pack, selected: &BTreeSet<OptionId>) -> InstallPlan {
    block_on(pack.plan(selected, PlanOptions::default(), &Offline)).unwrap()
}

fn archive(path: &Path, manifest: &Value, overrides: &[(&str, &str)]) {
    archive_with_marker(path, "modrinth.index.json", manifest, overrides);
}

fn archive_with_marker(path: &Path, marker: &str, manifest: &Value, overrides: &[(&str, &str)]) {
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    let options = SimpleFileOptions::default();
    for &(name, content) in overrides {
        zip.start_file(name, options).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.start_file(marker, options).unwrap();
    zip.write_all(manifest.to_string().as_bytes()).unwrap();
    zip.finish().unwrap();
}

#[test]
fn curseforge_preview_and_bundled_only_plan_use_the_same_plugin_contract() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("pack.zip");
    let manifest = json!({
        "manifestType": "minecraftModpack", "manifestVersion": 1, "name": "Example",
        "minecraft": {"version": "1.20.1", "modLoaders": [
            {"id": "forge-47.4.0"}, {"id": "neoforge-1.20.1-47.1.106", "primary": true}
        ]}, "files": [{"projectID": 2, "fileID": 3, "required": false}], "overrides": "payload"
    });
    archive_with_marker(
        &path,
        "manifest.json",
        &manifest,
        &[("payload/config/options.txt", "bundled")],
    );
    let pack = ModpackPlugin::builtin().inspect(&path).unwrap();
    assert_eq!(pack.source(), PackSource::CurseForge);
    assert_eq!(
        pack.game().loader(),
        Some((axlauncher_spi::LoaderKind::NeoForge, "1.20.1-47.1.106"))
    );
    assert_eq!(pack.optional_files().len(), 1);
    let plan = local_plan(&pack, &BTreeSet::new());
    assert!(plan.downloads().is_empty());
    let mut resolved = None;
    plan.write_bundled_files(|path, input| {
        let mut text = String::new();
        input.read_to_string(&mut text)?;
        resolved = Some((path.to_owned(), text));
        Ok(())
    })
    .unwrap();
    assert_eq!(
        resolved,
        Some(("config/options.txt".into(), "bundled".into()))
    );
}

fn manifest(files: Vec<Value>) -> Value {
    json!({
        "formatVersion": 1, "game": "minecraft", "versionId": "example-1",
        "name": "Example", "dependencies": { "minecraft": "1.21.1", "fabric-loader": "0.17.0" },
        "files": files,
    })
}

fn download(path: &str, client: &str) -> Value {
    json!({
        "path": path, "hashes": {
            "sha1": "a9993e364706816aba3e25717850c26c9cd0d89d", "sha512": hex::encode(Sha512::digest(b"abc"))
        },
        "downloads": ["https://cdn.modrinth.com/data/fixture.jar"], "fileSize": 3,
        "env": { "client": client, "server": "required" },
    })
}

#[test]
fn the_registered_reader_applies_client_selection_and_override_order() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("pack.mrpack");
    archive(
        &path,
        &manifest(vec![
            download("mods/required.jar", "required"),
            download("mods/optional.jar", "optional"),
            download("mods/server.jar", "unsupported"),
            download("mods/bundled.jar", "required"),
        ]),
        &[
            ("client-overrides/config/example.txt", "client"),
            ("overrides/config/example.txt", "common"),
            ("overrides/mods/bundled.jar", "bundled"),
            ("server-overrides/config/example.txt", "server"),
        ],
    );
    let pack = ModpackPlugin::builtin().inspect(&path).unwrap();
    assert_eq!(pack.source(), PackSource::Modrinth);
    assert_eq!(pack.game().minecraft(), "1.21.1");
    assert_eq!(
        pack.game().loader(),
        Some((axlauncher_spi::LoaderKind::Fabric, "0.17.0"))
    );
    assert_eq!(pack.required_count(), 1);
    let [optional] = pack.optional_files() else {
        panic!("expected one selectable file")
    };
    assert_eq!(optional.label(), "mods/optional.jar");
    assert_eq!(local_plan(&pack, &BTreeSet::new()).downloads().len(), 1);
    let plan = local_plan(&pack, &BTreeSet::from([optional.id().clone()]));
    assert_eq!(
        plan.downloads()
            .iter()
            .map(super::Download::path)
            .collect::<Vec<_>>(),
        ["mods/required.jar", "mods/optional.jar"]
    );
    let mut files = BTreeMap::new();
    plan.write_bundled_files(|path, content| {
        let mut text = String::new();
        content.read_to_string(&mut text)?;
        files.insert(path.to_owned(), text);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        files,
        BTreeMap::from([
            (String::from("config/example.txt"), String::from("client")),
            (String::from("mods/bundled.jar"), String::from("bundled"))
        ])
    );
}

#[test]
fn bad_destinations_and_case_aliases_are_refused_at_preview() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("unsafe.zip");
    for bad in [
        "../escape.jar",
        "mods/NUL.jar",
        "C:/escape.jar",
        ".AXLPROFILE",
        "config/a:stream",
    ] {
        archive(&path, &manifest(vec![download(bad, "required")]), &[]);
        assert!(
            matches!(
                ModpackPlugin::builtin().inspect(&path),
                Err(PackError::Invalid(_))
            ),
            "{bad}"
        );
    }
    archive(
        &path,
        &manifest(vec![download("Config/first", "required")]),
        &[("overrides/config/second", "second")],
    );
    assert!(matches!(
        ModpackPlugin::builtin().inspect(&path),
        Err(PackError::Invalid(_))
    ));
}

#[test]
fn an_unsupported_version_is_reported_before_its_schema_is_decoded() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("future.zip");
    archive(
        &path,
        &json!({"formatVersion": 2, "game": "minecraft"}),
        &[],
    );
    assert!(matches!(
        ModpackPlugin::builtin().inspect(&path),
        Err(PackError::Unsupported(_))
    ));
}

#[test]
fn the_pinned_download_checks_sha512_independently_of_the_manifest_size() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("pack.mrpack");
    for reported in [1_u64, 17_468, u64::MAX] {
        let mut entry = download("mods/test.jar", "required");
        entry["fileSize"] = json!(reported);
        archive(&path, &manifest(vec![entry]), &[]);
        let pack = ModpackPlugin::builtin().inspect(&path).unwrap();
        let selected = BTreeSet::new();
        let plan = local_plan(&pack, &selected);
        let file = &plan.downloads()[0];
        assert_eq!(file.size_hint(), Some(reported));
        assert!(file.verify(&b"abc"[..]).is_ok());
        assert_eq!(
            file.verify(&b"bad"[..]).unwrap_err().to_string(),
            "SHA-512 mismatch"
        );
        assert!(file.verify(&b"abcd"[..]).is_err());
    }
}
