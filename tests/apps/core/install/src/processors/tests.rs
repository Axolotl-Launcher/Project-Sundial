// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::processors::ProcessorError;
use crate::test_tempfile as tempfile;
use std::collections::BTreeMap;
use std::io::Write;

fn target() -> Target {
    Target {
        os: "windows".into(),
        arch: "x86_64".into(),
    }
}

fn version(
    processors: Vec<Processor>,
    data: BTreeMap<String, axlauncher_launch::version::DataEntry>,
) -> Version {
    Version {
        id: "1.20.2-neoforge".into(),
        kind: "release".into(),
        main_class: "cpw.mods.bootstraplauncher.BootstrapLauncher".into(),
        libraries: Vec::new(),
        arguments: None,
        minecraft_arguments: None,
        asset_index: None,
        assets: None,
        java_version: None,
        inherits_from: None,
        jar: Some("1.20.2".into()),
        downloads: None,
        logging: None,
        processors,
        data,
    }
}

fn entry(value: &str) -> axlauncher_launch::version::DataEntry {
    axlauncher_launch::version::DataEntry {
        client: value.into(),
        server: value.into(),
    }
}

/// Writes a jar with only a manifest, which is all the tool reader needs.
fn synthetic_jar(path: &Path, main_class: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    let file = std::fs::File::create(path).expect("jar");
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file(MANIFEST, options).expect("manifest");
    zip.write_all(format!("Main-Class: {main_class}\r\n").as_bytes())
        .expect("write");
    zip.finish().expect("finish");
}

#[test]
fn a_server_only_step_is_not_the_client_s() {
    let server = Processor {
        jar: "net.neoforged:installertools:1".into(),
        classpath: Vec::new(),
        args: Vec::new(),
        outputs: BTreeMap::new(),
        sides: Some(vec!["server".into()]),
    };
    let client = Processor {
        sides: Some(vec!["client".into()]),
        ..server.clone()
    };
    let both = Processor {
        sides: None,
        ..server.clone()
    };

    assert!(!applies(&server));
    assert!(applies(&client));
    assert!(applies(&both));
}

#[test]
fn a_coordinate_maps_to_its_maven_path() {
    assert_eq!(
        axlauncher_launch::version::maven_path("net.neoforged:neoform:1.20.2@zip").expect("path"),
        Path::new("net/neoforged/neoform/1.20.2/neoform-1.20.2.zip")
    );
}

#[test]
fn a_bracketed_argument_becomes_a_library_path() {
    let libraries = Path::new("libs");
    let version = version(Vec::new(), BTreeMap::new());

    let resolved = resolve_template(
        "[net.neoforged:neoform:1.20.2@zip]",
        libraries,
        &version,
        Path::new("root"),
    )
    .expect("resolved");

    assert!(resolved.ends_with("neoform-1.20.2.zip"), "{resolved}");
}

#[test]
fn launcher_values_are_available_to_a_step() {
    let version = version(Vec::new(), BTreeMap::new());

    let resolved = resolve_template(
        "{SIDE}/{MC}",
        Path::new("libs"),
        &version,
        Path::new("root"),
    );
    // `{MC}` is not a known name, so it stays visible.
    assert_eq!(resolved.expect("resolved"), "client/{MC}");
}

#[test]
fn the_jar_a_step_targets_is_the_one_the_profile_inherits() {
    let version = version(Vec::new(), BTreeMap::new());
    let jar = resolve_template(
        "{MINECRAFT_JAR}",
        Path::new("libs"),
        &version,
        Path::new("root"),
    )
    .expect("resolved");

    assert!(jar.ends_with("1.20.2.jar"), "{jar}");
}

#[test]
fn a_quoted_value_is_a_literal_not_a_path() {
    let version = version(
        Vec::new(),
        BTreeMap::from([("VERSION".into(), entry("'1.20.2'"))]),
    );

    let resolved = resolve_template("{VERSION}", Path::new("libs"), &version, Path::new("root"))
        .expect("resolved");

    assert_eq!(resolved, "1.20.2");
}

#[test]
fn a_binary_patcher_output_counts_as_current_when_it_exists() {
    let root = tempfile::tempdir().expect("temp");
    let output = root.path().join("patched.jar");
    std::fs::write(&output, b"jar").expect("output");

    let processor = Processor {
        jar: "net.neoforged.installertools:binarypatcher:1".into(),
        classpath: Vec::new(),
        args: vec![
            "--clean".into(),
            "{MC_SRG}".into(),
            "--output".into(),
            output.display().to_string(),
        ],
        outputs: BTreeMap::new(),
        sides: None,
    };
    let version = version(Vec::new(), BTreeMap::new());

    assert!(outputs_are_current(
        &processor,
        Path::new("libs"),
        root.path(),
        &version
    ));
}

#[test]
fn a_step_with_no_declared_or_inferable_output_always_runs() {
    let processor = Processor {
        jar: "net.neoforged.installertools:installertools:1".into(),
        classpath: Vec::new(),
        args: vec!["--task".into(), "MCP_DATA".into()],
        outputs: BTreeMap::new(),
        sides: None,
    };
    let version = version(Vec::new(), BTreeMap::new());

    assert!(!outputs_are_current(
        &processor,
        Path::new("libs"),
        Path::new("root"),
        &version
    ));
}

#[test]
fn a_tool_without_a_manifest_is_reported_rather_than_run() {
    let root = tempfile::tempdir().expect("temp");
    let jar = root.path().join("tool.jar");
    std::fs::write(&jar, b"not a jar").expect("jar");

    let error = read_main_class(&jar).expect_err("unreadable");
    assert!(matches!(error, ProcessorError::ToolUnreadable { .. }));
}

#[test]
fn a_tool_who_jars_manifest_is_found() {
    let root = tempfile::tempdir().expect("temp");
    let jar = root.path().join("tool.jar");
    synthetic_jar(&jar, "net.example.Tool");

    assert_eq!(read_main_class(&jar).expect("main"), "net.example.Tool");
}

#[test]
fn the_classpath_appends_the_tool_when_the_profile_omits_it() {
    let root = tempfile::tempdir().expect("temp");
    let libraries = root.path().join("libraries");
    let tool = libraries.join("net/example/tool/1/tool-1.jar");
    synthetic_jar(&tool, "net.example.Tool");

    let processor = Processor {
        jar: "net.example:tool:1".into(),
        classpath: Vec::new(),
        args: Vec::new(),
        outputs: BTreeMap::new(),
        sides: None,
    };

    let classpath =
        processor_classpath(&libraries, &tool, &processor, &target()).expect("classpath");

    assert_eq!(classpath, tool.display().to_string());
}
