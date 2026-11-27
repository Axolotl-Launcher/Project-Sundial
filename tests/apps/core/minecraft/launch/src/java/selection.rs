// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;

use super::{tests::*, *};

#[test]
fn a_managed_selection_falls_back_while_the_runtime_is_unpublished() {
    let root = TempDir::new();
    let managed = root.path().join("runtimes");
    let component = managed.join("java-runtime-delta");
    let fallback_home = root.path().join("jdk");
    let fallback = install_fixture(&fallback_home, "21.0.7");
    let provider =
        SystemJava::with_environment(Some(fallback_home), None).with_runtime_roots(vec![managed]);
    let saved = install_fixture(&component, "21.0.10");
    let provider = provider
        .with_registered(vec![saved.clone()])
        .with_chosen(vec![(21, saved.clone())]);
    let record = runtime_record(&component);
    fs::write(&record, b"{}").unwrap();
    assert_eq!(
        provider.executable_at(&saved, None, JavaMode::Console),
        Some(saved.clone())
    );

    fs::remove_file(record).unwrap();
    assert_eq!(
        provider.executable_at(&saved, None, JavaMode::Console),
        None
    );
    assert_eq!(
        provider.executable_for(21, JavaMode::Console),
        Some(fallback)
    );
    assert!(
        provider
            .discover()
            .iter()
            .all(|runtime| runtime.path() != saved)
    );
}

#[test]
fn automatic_selection_prefers_native_java_and_keeps_explicit_choices() {
    let root = TempDir::new();
    let native_home = root.path().join("native");
    let foreign_home = root.path().join("foreign");
    let native = install_fixture(&native_home, "21.0.7");
    let foreign = install_fixture(&foreign_home, "21.0.10");
    let host = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    fs::write(
        native_home.join("release"),
        format!("JAVA_VERSION=\"21.0.7\"\nOS_ARCH=\"{host}\"\n"),
    )
    .unwrap();
    fs::write(
        foreign_home.join("release"),
        "JAVA_VERSION=\"21.0.10\"\nOS_ARCH=\"sparcv9\"\n",
    )
    .unwrap();
    let provider = SystemJava::default().with_registered(vec![native.clone(), foreign.clone()]);

    assert_eq!(provider.executable_for(21, JavaMode::Console), Some(native));
    assert_eq!(
        provider.executable_at(&foreign, None, JavaMode::Console),
        Some(foreign.clone())
    );
    let pinned = provider.with_chosen(vec![(21, foreign.clone())]);
    assert_eq!(pinned.executable_for(21, JavaMode::Console), Some(foreign));
}

#[test]
fn a_native_bundled_runtime_still_ranks_above_a_foreign_standalone_runtime() {
    let root = TempDir::new();
    let product = root.path().join("IDE");
    let bundled_home = product.join("jbr");
    let standalone_home = root.path().join("standalone");
    let bundled = install_fixture(&bundled_home, "25.0.1");
    let standalone = install_fixture(&standalone_home, "25.0.9");
    let executable = bundled.file_name().unwrap().to_str().unwrap();
    fs::write(
        product.join("product-info.json"),
        format!(r#"{{"launch":[{{"javaExecutablePath":"jbr/bin/{executable}"}}]}}"#),
    )
    .unwrap();
    fs::write(
        bundled_home.join("release"),
        format!(
            "JAVA_VERSION=\"25.0.1\"\nOS_ARCH=\"{}\"\n",
            std::env::consts::ARCH
        ),
    )
    .unwrap();
    fs::write(
        standalone_home.join("release"),
        "JAVA_VERSION=\"25.0.9\"\nOS_ARCH=\"sparcv9\"\n",
    )
    .unwrap();
    let runtimes = [standalone, bundled.clone()].map(|path| JavaRuntime::inspect(&path).unwrap());

    assert_eq!(
        JavaRuntime::preferred_candidate(25, &runtimes)
            .unwrap()
            .path(),
        bundled
    );
}

#[test]
fn a_lookalike_does_not_hide_a_jdk_under_an_unrecognized_vendor() {
    let root = TempDir::new();
    let vendor = root.path().join("Example Vendor");
    let lookalike = java_in(&vendor.join("bin"), JavaMode::Console);
    let java = install_fixture(&vendor.join("Product 21"), "21.0.7");
    let mut paths = Vec::new();
    discovery::scan(root.path(), 3, &mut paths);

    assert!(discovery::inspect_automatic(&lookalike).is_none());
    assert!(paths.contains(&java));
}
