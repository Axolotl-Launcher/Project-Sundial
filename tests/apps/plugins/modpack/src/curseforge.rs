// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_spi::auth::{AuthNetwork, Body, Request, Response, TransportError};
use serde_json::json;

use super::{PinnedFile, api};
use crate::PlanOptions;
use crate::test_futures::executor::block_on;

const FILE: u32 = 1_234_005;
const PROJECT: u32 = 21;

struct Metadata {
    project: u32,
    url: Option<&'static str>,
}

impl AuthNetwork for Metadata {
    async fn send(&self, request: Request<'_>) -> Result<Response, TransportError> {
        assert_eq!(request.api_key(), Some("fixture-key"));
        let Some(Body::Json(body)) = request.body else {
            panic!("expected a JSON POST")
        };
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        let data = match request.url {
            "https://api.curseforge.com/v1/mods/files" => {
                assert_eq!(body, json!({"fileIds": [FILE]}));
                json!({"id": FILE, "modId": self.project, "gameId": 432,
                    "fileName": "shader+pack test.zip", "fileLength": 3, "isAvailable": true, "downloadUrl": self.url,
                    "hashes": [{"algo": 2, "value": "900150983cd24fb0d6963f7d28e17f72"}]})
            }
            "https://api.curseforge.com/v1/mods" => {
                assert_eq!(body, json!({"modIds": [PROJECT]}));
                json!({"id": PROJECT, "gameId": 432, "classId": 6552, "allowModDistribution": false,
                    "links": {"websiteUrl": "https://www.curseforge.com/minecraft/shaders/example/"}})
            }
            _ => panic!("unexpected CurseForge route"),
        };
        Ok(Response {
            status: 200,
            body: json!({"data": [data]}).to_string().into_bytes(),
        })
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
    let client = api::Client::new(&network, "fixture-key");
    let files =
        block_on(client.resolve(&[&pin()], PlanOptions::default().auto_download_restricted()))
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

    let manual = block_on(client.resolve(&[&pin()], false)).unwrap();
    assert!(manual[0].urls().is_empty());
    assert_eq!(manual[0].manual_url(), file.manual_url());
}

#[test]
fn metadata_must_match_the_pinned_project_and_foreign_urls_are_not_used() {
    let wrong_project = Metadata {
        project: 22,
        url: None,
    };
    let client = api::Client::new(&wrong_project, "fixture-key");
    assert!(block_on(client.resolve(&[&pin()], true)).is_err());

    let network = Metadata {
        project: PROJECT,
        url: Some("https://unrelated.example/shader.zip"),
    };
    let client = api::Client::new(&network, "fixture-key");
    let files = block_on(client.resolve(&[&pin()], true)).unwrap();
    assert!(files[0].urls()[0].starts_with("https://edge.forgecdn.net/"));
}
