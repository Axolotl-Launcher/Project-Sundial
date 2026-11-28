// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::processors::ProcessorError;
use crate::test_tempfile as tempfile;
use std::collections::BTreeMap;
use std::io::Write;

fn outputs_are_current(
    processor: &Processor,
    libraries: &Path,
    version: &Version,
    values: &[(&str, OsString)],
) -> bool {
    let game = GameDir::standalone(libraries.parent().unwrap());
    let outputs = output_paths(processor, &game, version, values).unwrap();
    !outputs.is_empty() && outputs.iter().all(|output| output.current(false))
}

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

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hash = sha1_smol::Sha1::new();
    hash.update(bytes);
    hash.digest().to_string()
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
        &version,
        &[],
    ));
}

#[test]
fn a_declared_output_whose_digest_does_not_match_is_not_current() {
    let root = tempfile::tempdir().expect("temp");
    let output = root.path().join("patched.jar");
    std::fs::write(&output, b"interrupted write").expect("output");
    let patched_sha = "0000000000000000000000000000000000000000";

    let processor = Processor {
        jar: "net.neoforged.installertools:binarypatcher:1".into(),
        classpath: Vec::new(),
        args: Vec::new(),
        outputs: BTreeMap::from([(output.display().to_string(), patched_sha.to_string())]),
        sides: None,
    };
    let version = version(Vec::new(), BTreeMap::new());

    // The file exists but is not what the installer said the step writes, so
    // the step that would repair it must run.
    assert!(!outputs_are_current(
        &processor,
        Path::new("libs"),
        &version,
        &[],
    ));
}

#[test]
fn a_declared_output_whose_digest_matches_is_current() {
    let root = tempfile::tempdir().expect("temp");
    let output = root.path().join("patched.jar");
    std::fs::write(&output, b"finished").expect("output");
    let sha = sha1_hex(b"finished");

    let processor = Processor {
        jar: "net.neoforged.installertools:binarypatcher:1".into(),
        classpath: Vec::new(),
        args: Vec::new(),
        outputs: BTreeMap::from([(output.display().to_string(), sha)]),
        sides: None,
    };
    let version = version(Vec::new(), BTreeMap::new());

    assert!(outputs_are_current(
        &processor,
        Path::new("libs"),
        &version,
        &[],
    ));
}

#[test]
fn a_digest_stated_as_a_template_is_resolved_before_the_check() {
    let root = tempfile::tempdir().expect("temp");
    let output = root.path().join("patched.jar");
    std::fs::write(&output, b"finished").expect("output");

    let processor = Processor {
        jar: "net.neoforged.installertools:binarypatcher:1".into(),
        classpath: Vec::new(),
        args: Vec::new(),
        outputs: BTreeMap::from([(output.display().to_string(), "{PATCHED_SHA}".into())]),
        sides: None,
    };
    let version = version(Vec::new(), BTreeMap::new());
    let values = [("PATCHED_SHA", sha1_hex(b"finished").into())];

    assert!(outputs_are_current(
        &processor,
        Path::new("libs"),
        &version,
        &values,
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
        &version,
        &[],
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

    assert_eq!(classpath, tool.as_os_str());
}
