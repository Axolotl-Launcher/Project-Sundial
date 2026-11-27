use axlauncher_launch::JavaRuntime;

use super::*;
use crate::scan::tests::{collect, java};

#[test]
fn exploration_follows_runtime_and_version_names_to_the_depth_limit() {
    let root = tempfile::tempdir().unwrap();
    let found = java(
        &root
            .path()
            .join("Minecraft/runtime/amd64/21.0.11/Temurin/1"),
    );
    java(
        &root
            .path()
            .join("Minecraft/runtime/amd64/21.0.11/Temurin/1/2"),
    );
    java(&root.path().join("Documents/runtime"));
    java(&root.path().join("NetEase Java/Temurin"));

    assert_eq!(
        collect(|scan| explore(scan, [root.path().to_owned()])),
        [dunce::canonicalize(found).unwrap()]
    );
}

#[test]
fn fixed_drive_seeds_use_installation_keywords() {
    assert!(is_installation_root(Path::new(r"D:\Eclipse Temurin")));
    assert!(is_installation_root(Path::new(r"D:\Java")));
    assert!(!is_installation_root(Path::new(r"D:\Runtime")));
    assert!(may_descend("runtime"));
    assert!(may_descend("21.0.11_4"));
    assert!(!may_descend("java8path"));
}

#[test]
fn program_files_searches_unrecognized_vendors_without_crossing_its_depth_limit() {
    let root = tempfile::tempdir().unwrap();
    let programs = dunce::canonicalize(root.path())
        .unwrap()
        .join("Program Files");
    let found = java(&programs.join("Acme/Java/jdk-21"));
    java(&programs.join("Acme/Java/Deep/jdk-25"));
    java(&programs.join("WpSystem/Java/jdk-25"));

    assert_eq!(
        collect(|scan| scan_program_files(scan, &[programs])),
        [found]
    );
}

#[test]
fn the_store_layout_accepts_runtime_components_on_non_wpsystem_paths() {
    let root = tempfile::tempdir().unwrap();
    let found = java(&root.path().join("java-runtime-epsilon/windows-x64/runtime"));
    java(&root.path().join("unrelated/windows-x64/runtime"));
    let excluded = root.path().join("WpSystem/runtime");
    java(&excluded.join("java-runtime-epsilon/windows-x64/runtime"));

    assert_eq!(
        collect(|scan| store_executables(scan, root.path())),
        [dunce::canonicalize(found).unwrap()]
    );
    assert!(collect(|scan| store_executables(scan, &excluded)).is_empty());
}

#[test]
fn windows_uses_fixed_drive_seeds_registry_homes_and_working_directory() {
    let root = tempfile::tempdir().unwrap();
    let drive = root.path().join("drive");
    let jdk = java(&drive.join("Eclipse Temurin"));
    java(&drive.join("unrelated"));
    let registry = root.path().join("registered installation");
    let installed = java(&registry);
    let working = root.path().join("working");
    let local = java(&working);

    let result = collect(|scanner| {
        scan(
            scanner,
            WindowsJavaPaths {
                drives: vec![drive],
                exploration_roots: Vec::new(),
                registry_homes: vec![registry],
                working_directory: Some(working.join("bin")),
                store_runtime_root: None,
            },
        )
    });
    assert_eq!(
        result,
        [dunce::canonicalize(jdk).unwrap(), installed, local]
    );
}

#[test]
fn exclusions_apply_to_the_reported_path_and_the_resolved_path() {
    let root = tempfile::tempdir().unwrap();
    let visible = root.path().join("Java");
    let excluded = root.path().join("wPsYsTeM");
    let hidden_runtime = java(&excluded.join("jdk"));
    let visible_runtime = java(&visible.join("jdk"));
    let visible_alias = root.path().join("outside");
    let hidden_alias = excluded.join("alias");
    junction(&visible_alias, &excluded);
    junction(&hidden_alias, &visible);

    for (alias, original) in [
        (visible_alias.join("jdk/bin/java.exe"), hidden_runtime),
        (hidden_alias.join("jdk/bin/java.exe"), visible_runtime),
    ] {
        let runtime = JavaRuntime::inspect(&alias).unwrap();
        assert_eq!(runtime.path(), dunce::canonicalize(original).unwrap());
        assert!(JavaRuntime::inspect_automatic(&alias).is_none());
    }
    assert!(collect(|scan| scan.conventional_roots([visible_alias, hidden_alias])).is_empty());
}

fn junction(alias: &Path, destination: &Path) {
    assert!(
        std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(alias)
            .arg(destination)
            .output()
            .unwrap()
            .status
            .success()
    );
}
