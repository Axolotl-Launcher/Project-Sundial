// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! An installer's own tool must not shadow the game library of the same name.

use super::*;
use crate::version::{Artifact, LibraryDownloads};

fn target() -> Target {
    Target {
        os: "linux".into(),
        arch: "x86_64".into(),
    }
}

fn library(name: &str, path: &str, on_classpath: bool) -> Library {
    Library {
        name: name.into(),
        downloads: Some(LibraryDownloads {
            artifact: Some(Artifact {
                path: path.into(),
                sha1: None,
                size: None,
                url: None,
            }),
            classifiers: None,
        }),
        natives: None,
        rules: None,
        url: None,
        include_in_classpath: on_classpath,
    }
}

#[test]
fn a_tool_that_shares_a_coordinate_does_not_remove_the_game_library() {
    let game = library("org.ow2.asm:asm:9.6", "game/asm-9.6.jar", true);
    let tool = library("org.ow2.asm:asm:9.6", "tool/asm-9.6.jar", false);

    let classpath = build(Path::new("l"), &[game, tool], Path::new("c.jar"), &target());

    assert!(classpath.contains("game/asm-9.6.jar"), "{classpath}");
    assert!(!classpath.contains("tool/asm-9.6.jar"), "{classpath}");
}
