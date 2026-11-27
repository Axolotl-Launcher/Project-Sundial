use std::fs::File;
use std::io::Write;
use std::path::Path;

use axlauncher_core::loader;
use axlauncher_core::network_spi::{FetchError, Network};
use axlauncher_core::spi::{LoaderError, LoaderKind, LoaderRequest, ModLoader};

const MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/forge";
const COORDINATE: &str = "1.20.1-47.1.5";
const EXPECTED_INSTALLER: &str = "fixture installer requested";

struct PublishedBuild;

impl Network for PublishedBuild {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        if url == format!("{MAVEN}/maven-metadata.xml") {
            return Ok(format!(
                "<metadata><versioning><versions><version>{COORDINATE}</version></versions></versioning></metadata>"
            )
            .into_bytes());
        }
        assert_eq!(
            url,
            format!("{MAVEN}/{COORDINATE}/forge-{COORDINATE}-installer.jar")
        );
        Err(FetchError::Fatal(EXPECTED_INSTALLER.into()))
    }
}

fn curseforge_pack(path: &Path, build: &str) {
    let mut writer = zip::ZipWriter::new(File::create(path).unwrap());
    writer
        .start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    write!(
        writer,
        r#"{{
            "name": "Version fixture", "manifestType": "minecraftModpack", "manifestVersion": 1,
            "minecraft": {{"version": "1.20.1", "modLoaders": [{{"id": "neoforge-{build}", "primary": true}}]}}
        }}"#
    )
    .unwrap();
    writer.finish().unwrap();
}

#[test]
fn an_imported_neoforge_build_requests_the_same_artifact_as_the_picker() {
    let temp = tempfile::tempdir().unwrap();
    let provider = loader::provider(PublishedBuild, LoaderKind::NeoForge).unwrap();
    let listing = futures::executor::block_on(provider.versions("1.20.1")).unwrap();
    assert_eq!(listing[0].version, COORDINATE);

    for (filename, build) in [("complete.zip", COORDINATE), ("short.zip", "47.1.5")] {
        let path = temp.path().join(filename);
        curseforge_pack(&path, build);
        let pack = super::inspect(&path).unwrap();
        let (kind, imported) = pack.game().loader().unwrap();
        assert_eq!((kind, imported), (LoaderKind::NeoForge, build));
        let error = futures::executor::block_on(provider.profile(LoaderRequest {
            game_version: pack.game().minecraft().into(),
            loader_version: Some(imported.into()),
        }))
        .unwrap_err();
        assert!(matches!(
            error,
            LoaderError::Unavailable { source: FetchError::Fatal(reason), .. }
                if reason == EXPECTED_INSTALLER
        ));
    }
}
