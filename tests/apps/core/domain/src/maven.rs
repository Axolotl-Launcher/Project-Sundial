// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn a_three_part_coordinate_is_a_jar() {
    assert_eq!(
        path("org.ow2.asm:asm:9.10.1").expect("path"),
        PathBuf::from("org/ow2/asm/asm/9.10.1/asm-9.10.1.jar")
    );
}

#[test]
fn a_classifier_is_part_of_the_file_name() {
    assert_eq!(
        path("net.minecraftforge:forge:1.20.1-47.4.28:client").expect("path"),
        PathBuf::from("net/minecraftforge/forge/1.20.1-47.4.28/forge-1.20.1-47.4.28-client.jar")
    );
}

#[test]
fn an_extension_replaces_jar() {
    assert_eq!(
        path("de.oceanlabs.mcp:mcp_config:1.20.1:mappings@txt").expect("path"),
        PathBuf::from("de/oceanlabs/mcp/mcp_config/1.20.1/mcp_config-1.20.1-mappings.txt")
    );
}

#[test]
fn an_incomplete_coordinate_has_no_path() {
    assert_eq!(path("org.example"), None);
    assert_eq!(path("org.example:artifact"), None);
    assert_eq!(path("::1"), None);
    assert_eq!(path(""), None);
}

#[test]
fn a_coordinate_that_escapes_the_repository_has_no_path() {
    assert_eq!(path("../../evil:artifact:1"), None);
    assert_eq!(path("org.example:artifact:1/../../.."), None);
    assert_eq!(path("org.example:artifact:1.0:../../x"), None);
    assert_eq!(path("org.example:artifact:1@../../x"), None);
    assert_eq!(path("org.example:artifact:1:classifier:extra"), None);
    assert_eq!(path("org..example:artifact:1"), None);
}
