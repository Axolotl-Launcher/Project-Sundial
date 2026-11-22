// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::Path;

use axlauncher_core::storage::DataDir;

use super::*;

#[test]
fn launch_paths_use_the_instances_and_versions_directories() {
    let root = Path::new("data");
    let paths = launch_paths(root, "instance-3");
    let data = DataDir::new(root);

    assert_eq!(paths.game_root, root.to_path_buf());
    assert_eq!(
        paths.instance_directory,
        data.instances().join("instance-3")
    );
    assert_eq!(paths.versions_root, data.versions());
    assert_ne!(paths.instance_directory, root.join("instance-3"));
}
