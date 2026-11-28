// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_spi::curseforge::{CatalogError, File, Mod, ModpackCatalog};
use serde_json::json;

use super::{PinnedFile, resolve};
use crate::PlanOptions;
use crate::test_futures::executor::block_on;

const FILE: u32 = 1_234_005;
const PROJECT: u32 = 21;

struct Metadata {
    project: u32,
    url: Option<&'static str>,
}

impl ModpackCatalog for Metadata {
    async fn get_files(&self, ids: &[u32]) -> Result<Vec<File>, CatalogError> {
        assert_eq!(ids, [FILE]);
        Ok(serde_json::from_value(json!([{
            "id": FILE, "modId": self.project, "gameId": 432,
            "fileName": "shader+pack test.zip", "displayName": "Shader Pack", "fileLength": 3,
            "isAvailable": true, "downloadUrl": self.url, "gameVersions": ["1.21"],
            "hashes": [{"algo": 2, "value": "900150983cd24fb0d6963f7d28e17f72"}]
        }]))
        .unwrap())
    }

    async fn get_mods(&self, ids: &[u32]) -> Result<Vec<Mod>, CatalogError> {
        assert_eq!(ids, [PROJECT]);
        Ok(serde_json::from_value(json!([{
            "id": PROJECT, "gameId": 432, "classId": 6552, "allowModDistribution": false,
            "name": "Shader Pack", "slug": "example", "summary": "Test fixture.",
            "links": {"websiteUrl": "https://www.curseforge.com/minecraft/shaders/example/"}
        }]))
        .unwrap())
    }
}

fn pin() -> PinnedFile {
    PinnedFile {
        project_id: PROJECT,
        file_id: FILE,
        required: true,
    }
}

#[test]
fn restricted_files_use_the_official_cdn_by_default_and_keep_a_manual_page() {
    let network = Metadata {
        project: PROJECT,
        url: None,
    };
    let files = block_on(resolve::downloads(
        &network,
        &[&pin()],
        PlanOptions::default().auto_download_restricted(),
    ))
    .unwrap();
    let file = &files[0];
    assert_eq!(file.path(), "shaderpacks/shader+pack test.zip");
    assert_eq!(
        file.urls(),
        ["https://edge.forgecdn.net/files/1234/5/shader%2Bpack%20test.zip"]
    );
    assert_eq!(
        file.manual_url(),
        Some("https://www.curseforge.com/minecraft/shaders/example/download/1234005")
    );
    assert!(file.verify(&b"abc"[..]).is_ok());
    assert!(file.verify(&b"bad"[..]).is_err());

    let manual = block_on(resolve::downloads(&network, &[&pin()], false)).unwrap();
    assert!(manual[0].urls().is_empty());
    assert_eq!(manual[0].manual_url(), file.manual_url());
}

#[test]
fn metadata_must_match_the_pinned_project_and_foreign_urls_are_not_used() {
    let wrong_project = Metadata {
        project: 22,
        url: None,
    };
    assert!(block_on(resolve::downloads(&wrong_project, &[&pin()], true)).is_err());

    let network = Metadata {
        project: PROJECT,
        url: Some("https://unrelated.example/shader.zip"),
    };
    let files = block_on(resolve::downloads(&network, &[&pin()], true)).unwrap();
    assert!(files[0].urls()[0].starts_with("https://edge.forgecdn.net/"));
}
