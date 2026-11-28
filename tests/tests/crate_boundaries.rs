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

fn provider_contract(package: &str) -> bool {
    matches!(
        package,
        "axlauncher_spi" | "axlauncher_download_spi" | "axlauncher_network_spi"
    )
}

fn core_system(package: &str, directory: &Path) -> bool {
    package == "axlauncher_worker"
        || provider_contract(package)
        || directory.parent() == Some(Path::new("apps/core"))
        || (matches!(package, "axlauncher_mod_metadata" | "axlauncher_resources")
            && directory.parent() == Some(Path::new("apps/core/content")))
}

fn permitted_edge(owner: &str, dependency: &str, target_dir: &Path) -> bool {
    match owner {
        "axlauncher_domain"
        | "axlauncher_ui"
        | "axlauncher_policy_tests"
        | "axlauncher_storage_layout"
        | "axlauncher_network_spi" => false,
        "axlauncher_download_spi" => dependency == "axlauncher_network_spi",
        "axlauncher_spi" => matches!(dependency, "axlauncher_domain" | "axlauncher_network_spi"),
        "axlauncher_java_discovery" => matches!(
            dependency,
            "axlauncher_launch" | "axlauncher_storage" | "axlauncher_worker"
        ),
        "axlauncher_modrinth" => matches!(dependency, "axlauncher_network" | "axlauncher_worker"),
        "axlauncher_storage" => {
            matches!(dependency, "axlauncher_spi" | "axlauncher_storage_layout")
        }
        "axlauncher_core" => core_system(dependency, target_dir),
        "axlauncher_content_render" => dependency == "axlauncher_ui",
        "axlauncher_frontend" => matches!(
            dependency,
            "axlauncher_ui"
                | "axlauncher_core"
                | "axlauncher_content_render"
                | "axlauncher_modpack"
        ),
        "axlauncher_modpack" => provider_contract(dependency),
        "axlauncher_mmc_convert" => matches!(
            dependency,
            "axlauncher_core"
                | "axlauncher_modpack"
                | "axlauncher_download_engine"
                | "axlauncher_install_runtime"
        ),
        "axlauncher_platform_windows"
        | "axlauncher_platform_linux"
        | "axlauncher_platform_macos" => dependency == "axlauncher_frontend",
        _ => implementation_edge(owner, dependency),
    }
}

fn implementation_edge(owner: &str, dependency: &str) -> bool {
    match dependency {
        "axlauncher_domain" => true,
        contract if provider_contract(contract) => true,
        "axlauncher_storage" => matches!(
            owner,
            "axlauncher_download_engine"
                | "axlauncher_accounts"
                | "axlauncher_worker"
                | "axlauncher_install"
                | "axlauncher_install_runtime"
                | "axlauncher_state"
                | "axlauncher_launch"
        ),
        "axlauncher_launch" => {
            matches!(owner, "axlauncher_install" | "axlauncher_install_runtime")
        }
        "axlauncher_install" => {
            matches!(owner, "axlauncher_worker" | "axlauncher_install_runtime")
        }
        "axlauncher_loader" => owner == "axlauncher_install_runtime",
        "axlauncher_worker" => {
            matches!(
                owner,
                "axlauncher_accounts" | "axlauncher_download_engine" | "axlauncher_install_runtime"
            )
        }
        "axlauncher_mc_version" => owner == "axlauncher_loader",
        "axlauncher_network" => {
            matches!(
                owner,
                "axlauncher_download_engine"
                    | "axlauncher_install"
                    | "axlauncher_install_runtime"
                    | "axlauncher_accounts"
                    | "axlauncher_worker"
            )
        }
        _ => false,
    }
}

fn resource_owner(dependency: &str) -> Option<&'static str> {
    match dependency {
        "reqwest" => Some("axlauncher_network"),
        "libloading" => Some("axlauncher_worker"),
        "keyring" | "sysinfo" | "windows-registry" => Some("axlauncher_storage"),
        "gpui-kit" => Some("axlauncher_ui"),
        _ => None,
    }
}

fn dependency_allowed(owner: &str, name: &str, directory: Option<&Path>) -> bool {
    if owner == "axlauncher_instance_settings" {
        return matches!(name, "axlauncher_domain" | "axlauncher_worker");
    }
    if matches!(
        name,
        "dirs" | "dirs-next" | "directories" | "directories-next"
    ) {
        return matches!(owner, "axlauncher_storage" | "axlauncher_storage_layout");
    }
    if let Some(resource_owner) = resource_owner(name) {
        return owner == resource_owner;
    }
    directory.is_none_or(|directory| permitted_edge(owner, name, directory))
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
            let name = dependency.name.as_str();
            let allowed = dependency_allowed(&package.name, name, directories.get(name).copied());
            assert!(
                allowed,
                "{} -> {} crosses a resource boundary. See .agents/docs/crates.md",
                package.name, dependency.name
            );
        }
    }
}
