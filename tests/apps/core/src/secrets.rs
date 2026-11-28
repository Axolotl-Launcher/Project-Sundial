use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

use axlauncher_modpack::{ModpackPlugin, PlanOptions};
use axlauncher_network_spi::auth::{AuthNetwork, Body, Request, Response, TransportError};

use crate::desktop::curseforge::{DesktopCurseForge, build_catalog};

use super::*;

fn candidate(symbol: char) -> String {
    std::iter::repeat_n(symbol, 32).collect()
}

fn write_value(root: &Path, file: &str, value: &str) {
    fs::write(root.join(file), format!("{KEY_NAME}={value}")).unwrap();
}

#[test]
fn process_overrides_runtime_files_which_override_the_compiled_default() {
    let root = tempfile::tempdir().unwrap();
    let defaults = candidate('d');
    let dot_env = candidate('e');
    let local = candidate('l');
    let mode = candidate('m');
    let process = candidate('p');
    let verify = |process: Option<&str>, expected: &str| {
        let key = resolve(process, root.path(), Some(&defaults)).unwrap();
        assert!(key.expose_secret() == expected);
    };
    verify(None, &defaults);
    write_value(root.path(), ".env", &dot_env);
    verify(None, &dot_env);
    write_value(root.path(), RUNTIME_FILES[2], &mode);
    verify(None, &mode);
    write_value(root.path(), ".env.local", &local);
    verify(None, &local);
    let most_specific = "$".to_owned() + &candidate('s');
    fs::write(
        root.path().join(RUNTIME_FILES[0]),
        format!("\u{feff} export {KEY_NAME} = '{most_specific}'  # comment\n"),
    )
    .unwrap();
    verify(Some(" \t"), &most_specific);
    verify(Some(&process), &process);
}

#[test]
fn missing_empty_and_invalid_keys_are_distinct_and_secret_debug_is_redacted() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        resolve(None, root.path(), None),
        Err(CatalogError::MissingApiKey)
    );
    write_value(root.path(), ".env", "");
    assert_eq!(
        resolve(Some(""), root.path(), Some("")),
        Err(CatalogError::MissingApiKey)
    );
    write_value(root.path(), ".env", "'unterminated");
    let fallback = candidate('d');
    assert_eq!(
        resolve(None, root.path(), Some(&fallback)),
        Err(CatalogError::InvalidApiKey),
    );
    assert_eq!(
        resolve(Some("a\r\nb"), root.path(), Some(&fallback)),
        Err(CatalogError::InvalidApiKey),
    );
    let key = resolve(Some(&fallback), root.path(), None).unwrap();
    assert_eq!(format!("{key:?}"), "ApiKey([redacted])");
    assert!(!format!("{:?}", Some(key)).contains(&fallback));
}

#[test]
fn missing_key_disables_the_catalog_before_constructing_http() {
    let empty = tempfile::tempdir().unwrap();
    let result = resolve(None, empty.path(), None);
    let client = build_catalog::<&MigrationHttp>(result.as_ref().map_err(|error| *error), || {
        panic!("a disabled provider must not create a transport");
    });
    assert!(matches!(client, Err(CatalogError::MissingApiKey)));
}

struct MigrationHttp {
    resolved: ApiKey,
    requests: AtomicUsize,
}

impl AuthNetwork for &MigrationHttp {
    async fn send(&self, request: Request<'_>) -> Result<Response, TransportError> {
        assert!(request.api_key() == Some(&self.resolved));
        self.requests.fetch_add(1, Ordering::Relaxed);
        let resource = match (request.url, request.body) {
            ("https://api.curseforge.com/v1/mods/24", None) => MOD.to_owned(),
            ("https://api.curseforge.com/v1/mods", Some(Body::Json(body))) => {
                assert_eq!(body, r#"{"modIds":[24]}"#);
                format!("[{MOD}]")
            }
            ("https://api.curseforge.com/v1/mods/files", Some(Body::Json(body))) => {
                assert_eq!(body, r#"{"fileIds":[17]}"#);
                format!("[{FILE}]")
            }
            _ => panic!("unexpected catalog request"),
        };
        Ok(Response::new(
            200,
            format!(r#"{{"data":{resource}}}"#).into_bytes(),
        ))
    }
}

const MOD: &str = r#"{"id":24,"gameId":432,"name":"Test","slug":"test","summary":"Example",
    "classId":6,"links":{"websiteUrl":"https://www.curseforge.com/minecraft/mc-mods/test"}}"#;
const FILE: &str = r#"{"id":17,"modId":24,"gameId":432,"displayName":"Test","fileName":"test.jar",
    "fileLength":3,"isAvailable":true,"gameVersions":["1.21"],
    "downloadUrl":"https://edge.forgecdn.net/files/0/17/test.jar"}"#;

#[tokio::test]
async fn public_lookup_and_import_plan_use_the_same_runtime_override() {
    let root = tempfile::tempdir().unwrap();
    let runtime = candidate('r');
    let fallback = candidate('d');
    write_value(root.path(), ".env.local", &runtime);
    let resolved = resolve(None, root.path(), Some(&fallback)).unwrap();
    let network = MigrationHttp {
        resolved: ApiKey::new(&runtime).unwrap(),
        requests: AtomicUsize::new(0),
    };
    let client = build_catalog(Ok(&resolved), || Ok(&network)).unwrap();
    assert_eq!(client.get_mod(24).await.unwrap().id, 24);
    let archive = root.path().join("pack.zip");
    write_pack(&archive, r#"[{"projectID":24,"fileID":17}]"#);
    let pack = ModpackPlugin::builtin().inspect(&archive).unwrap();
    let plan = pack
        .plan(&BTreeSet::new(), PlanOptions::default(), &client)
        .await
        .unwrap();
    assert_eq!(plan.downloads()[0].path(), "mods/test.jar");
    assert_eq!(network.requests.load(Ordering::Relaxed), 3);
}

#[tokio::test]
async fn a_missing_catalog_key_does_not_affect_bundled_curseforge_imports() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("pack.zip");
    write_pack(&archive, "[]");
    let pack = ModpackPlugin::builtin().inspect(&archive).unwrap();
    let plan = pack
        .plan(&BTreeSet::new(), PlanOptions::default(), &DesktopCurseForge)
        .await
        .unwrap();
    assert!(plan.downloads().is_empty());
}

fn write_pack(path: &Path, files: &str) {
    let mut archive = zip::ZipWriter::new(fs::File::create(path).unwrap());
    archive
        .start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    write!(
        archive,
        r#"{{"manifestType":"minecraftModpack","manifestVersion":1,"name":"Test",
        "minecraft":{{"version":"1.21"}},"files":{files}}}"#
    )
    .unwrap();
    archive.finish().unwrap();
}

#[tokio::test]
#[ignore = "Requires an explicitly configured CURSEFORGE_API_KEY in the process environment"]
async fn live_curseforge_catalog_only_when_process_key_is_present() {
    if validate(process_key().unwrap().as_deref())
        .unwrap()
        .is_none()
    {
        eprintln!("Skipped live CurseForge smoke: CURSEFORGE_API_KEY is not set.");
        return;
    }
    assert!(!DesktopCurseForge.categories(None).await.unwrap().is_empty());
}
