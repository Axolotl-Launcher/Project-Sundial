// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Cargo metadata exposes the original package name even for renamed, optional
//! and target-specific dependencies that are inactive on the test host.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const GUIDE: &str = include_str!("../../.agents/docs/crates.md");

#[derive(Deserialize)]
struct Workspace {
    packages: Vec<Package>,
    workspace_root: PathBuf,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
}

fn workspace() -> Workspace {
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version=1",
            "--locked",
            "--no-deps",
            "--offline",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("Cargo workspace metadata")
}

fn permitted_edge(owner: &str, dependency: &str, target_dir: &Path) -> bool {
    match owner {
        "axlauncher_domain" | "axlauncher_ui" | "axlauncher_policy_tests" => false,
        "axlauncher_spi" => dependency == "axlauncher_domain",
        "axlauncher_core" => target_dir.parent() == Some(Path::new("apps/core")),
        "axlauncher_frontend" => matches!(
            dependency,
            "axlauncher_ui" | "axlauncher_core" | "axlauncher_modpack"
        ),
        "axlauncher_modpack" => dependency == "axlauncher_spi",
        "axlauncher_mmc_convert" => matches!(dependency, "axlauncher_core" | "axlauncher_modpack"),
        "axlauncher_platform_windows"
        | "axlauncher_platform_linux"
        | "axlauncher_platform_macos" => dependency == "axlauncher_frontend",
        _ => match dependency {
            "axlauncher_spi" | "axlauncher_domain" => true,
            "axlauncher_storage" => matches!(
                owner,
                "axlauncher_download_engine"
                    | "axlauncher_install"
                    | "axlauncher_state"
                    | "axlauncher_launch"
            ),
            "axlauncher_launch" | "axlauncher_download_engine" => owner == "axlauncher_install",
            "axlauncher_network" => {
                matches!(owner, "axlauncher_download_engine" | "axlauncher_install")
            }
            _ => false,
        },
    }
}

#[test]
fn production_dependencies_respect_crate_boundaries() {
    let workspace = workspace();
    let directories: HashMap<_, _> = workspace
        .packages
        .iter()
        .map(|package| {
            let manifest = package
                .manifest_path
                .strip_prefix(&workspace.workspace_root)
                .expect("workspace member");
            (
                package.name.as_str(),
                manifest.parent().expect("crate directory"),
            )
        })
        .collect();

    for package in &workspace.packages {
        assert!(
            GUIDE.contains(&format!("`{}`", package.name)),
            "Document {} in .agents/docs/crates.md",
            package.name
        );
        for dependency in package
            .dependencies
            .iter()
            .filter(|dep| dep.kind.as_deref() != Some("dev"))
        {
            let allowed = match dependency.name.as_str() {
                "reqwest" => package.name == "axlauncher_network",
                "keyring" => package.name == "axlauncher_storage",
                "gpui-kit" => package.name == "axlauncher_ui",
                name => directories
                    .get(name)
                    .is_none_or(|directory| permitted_edge(&package.name, name, directory)),
            };
            assert!(
                allowed,
                "{} -> {} crosses a resource boundary. See .agents/docs/crates.md",
                package.name, dependency.name
            );
        }
    }
}
