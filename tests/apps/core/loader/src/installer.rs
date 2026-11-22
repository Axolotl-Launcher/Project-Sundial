// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io::{Cursor, Write};

use super::*;

const TOOL_JAR: &[u8] = b"tool";
const BINPATCH: &[u8] = b"patch";

/// An installer with the parts every modern one carries.
fn installer(game_version: &str) -> Vec<u8> {
    let install = format!(
        r#"{{
  "spec": 1,
  "version": "1.20.1-forge-1",
  "minecraft": "{game_version}",
  "json": "/version.json",
  "libraries": [
    {{
      "name": "net.minecraftforge:installertools:1.0",
      "downloads": {{
        "artifact": {{
          "path": "net/minecraftforge/installertools/1.0/installertools-1.0.jar",
          "url": "https://example.test/tool.jar",
          "sha1": "0000000000000000000000000000000000000000",
          "size": 4
        }}
      }}
    }}
  ],
  "processors": [
    {{ "jar": "net.minecraftforge:installertools:1.0", "classpath": [], "args": ["-i", "{{BINPATCH}}"] }}
  ],
  "data": {{
    "BINPATCH": {{ "client": "/data/client.lzma", "server": "/data/server.lzma" }},
    "MAPPINGS": {{ "client": "[de.oceanlabs.mcp:mcp_config:1:mappings@txt]", "server": "[de.oceanlabs.mcp:mcp_config:1:mappings@txt]" }}
  }}
}}"#
    );
    let version = format!(
        r#"{{
  "id": "1.20.1-forge-1",
  "inheritsFrom": "{game_version}",
  "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
  "libraries": [],
  "arguments": {{ "game": [], "jvm": [] }}
}}"#
    );

    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in [
        ("install_profile.json".to_string(), install.into_bytes()),
        ("version.json".to_string(), version.into_bytes()),
        ("data/client.lzma".to_string(), BINPATCH.to_vec()),
        ("data/server.lzma".to_string(), BINPATCH.to_vec()),
        (
            "maven/net/example/tool/1/tool-1.jar".to_string(),
            TOOL_JAR.to_vec(),
        ),
    ] {
        writer.start_file(name, options).expect("entry");
        writer.write_all(&bytes).expect("write");
    }
    writer.finish().expect("zip").into_inner()
}

#[test]
fn an_installer_becomes_one_self_contained_profile() {
    let bytes = installer("1.20.1");

    let profile = parse(LoaderKind::Forge, &bytes, "1.20.1").expect("profile");

    assert_eq!(profile.version_id, "1.20.1-forge-1");
    let document: Value = serde_json::from_slice(&profile.document).expect("document");
    assert_eq!(document["processors"].as_array().map(Vec::len), Some(1));
    let libraries = document["libraries"].as_array().expect("libraries");
    assert_eq!(libraries.len(), 1);
    assert_eq!(libraries[0]["include_in_classpath"], false);

    let binpatch = document["data"]["BINPATCH"]["client"]
        .as_str()
        .expect("client data");
    assert!(binpatch.starts_with("[axlauncher.embedded:"), "{binpatch}");
    assert_eq!(
        document["data"]["MAPPINGS"]["client"], "[de.oceanlabs.mcp:mcp_config:1:mappings@txt]",
        "a coordinate is already one"
    );
}

#[test]
fn the_files_the_installer_lifts_out_travel_with_the_profile() {
    let bytes = installer("1.20.1");

    let profile = parse(LoaderKind::Forge, &bytes, "1.20.1").expect("profile");

    let paths: Vec<&str> = profile
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert!(
        paths.iter().any(|path| path.ends_with("client.lzma")),
        "{paths:?}"
    );
    assert!(
        !paths.iter().any(|path| path.ends_with("server.lzma")),
        "a server input is not part of a client install: {paths:?}"
    );
    assert!(
        paths.contains(&"libraries/net/example/tool/1/tool-1.jar"),
        "{paths:?}"
    );
    for file in &profile.files {
        assert!(file.path.starts_with("libraries/"), "{}", file.path);
        assert!(!file.bytes.is_empty(), "{}", file.path);
    }
}

#[test]
fn an_installer_for_another_game_version_is_refused() {
    let bytes = installer("1.20.1");

    let error = parse(LoaderKind::Forge, &bytes, "1.20.2").expect_err("mismatch");

    assert!(matches!(error, LoaderError::Malformed { .. }), "{error}");
}

#[test]
fn something_that_is_not_an_installer_is_malformed() {
    let error = parse(LoaderKind::Forge, b"not a zip", "1.20.1").expect_err("not a zip");

    assert!(matches!(error, LoaderError::Malformed { .. }), "{error}");
}
