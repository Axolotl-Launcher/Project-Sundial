// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::java::tests::{TempDir, install_fixture};
use crate::platform::name;
use axlauncher_spi::JavaMode;

#[test]
fn the_product_must_declare_the_exact_executable() {
    let root = TempDir::new();
    let product = root.path().join("IDE");
    let jbr = install_fixture(&product.join("jbr"), "25.0.9");
    let other = install_fixture(&product.join("other"), "25.0.1");
    let metadata = product.join("product-info.json");
    fs::write(&metadata, b"{broken").unwrap();
    assert!(!owns_runtime(&jbr));

    let executable = name(JavaMode::Console);
    fs::write(
        metadata,
        format!(r#"{{"launch":[{{}},{{"javaExecutablePath":"jbr/bin/{executable}"}}]}}"#),
    )
    .unwrap();
    assert!(owns_runtime(&jbr));
    assert!(!owns_runtime(&other));
}

#[test]
fn a_macos_product_declares_java_relative_to_its_resources() {
    let root = TempDir::new();
    let contents = root.path().join("IDE.app/Contents");
    let resources = contents.join("Resources");
    fs::create_dir_all(&resources).unwrap();
    let jbr = install_fixture(&contents.join("jbr/Contents/Home"), "25.0.9");
    let executable = name(JavaMode::Console);
    fs::write(
        resources.join("product-info.json"),
        format!(
            r#"{{"launch":[{{"javaExecutablePath":"../jbr/Contents/Home/bin/{executable}"}}]}}"#
        ),
    )
    .unwrap();
    assert!(owns_runtime(&jbr));
}
