// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use markdown::{ParseOptions, to_mdast};

use super::{Block, parse_block};

#[test]
fn extracts_a_centered_comparison_image_and_its_alt_text() {
    let markdown = "<center>\n <img src=\"https://cdn.modrinth.com/data/comparison.png\" alt=\"Before &amp; after\"></img>\n <p>\n <!-- Adds a margin -->\n </p>\n</center>\n\nExplanation";
    let blocks = special_blocks(markdown);
    let [Block::Image(image)] = blocks.as_slice() else {
        panic!("expected a comparison image, got {blocks:?}");
    };
    assert!(image.is_centered());
    assert_eq!(image.src(), "https://cdn.modrinth.com/data/comparison.png");
    assert_eq!(image.alt(), "Before & after");
}

#[test]
fn preserves_a_standalone_badges_height() {
    let blocks =
        special_blocks(r#"<img height="42" src="https://cdn.modrinth.com/data/badge.webp"></img>"#);
    let [Block::Image(image)] = blocks.as_slice() else {
        panic!("expected a badge, got {blocks:?}");
    };
    assert!(!image.is_centered());
    assert_eq!(image.height(), Some(42));
    assert_eq!(image.width(), None);
}

#[test]
fn keeps_markdown_emphasis_in_superscript_notes() {
    let markdown = "<sup>_**Hardware:** 微星._</sup>\n<sup>_**Software:** Linux._</sup>";
    let blocks = special_blocks(markdown);
    let [Block::Superscript(notes)] = blocks.as_slice() else {
        panic!("expected a note block, got {blocks:?}");
    };
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].markdown(), "_**Hardware:** 微星._");
    assert_eq!(notes[0].text(), "Hardware: 微星.");
    assert_eq!(notes[1].text(), "Software: Linux.");
}

#[test]
fn leaves_headings_inline_images_and_code_with_the_standard_renderer() {
    let markdown = "<h1 id=\"installation\">Installation</h1>\n\nAvailable on ![Fabric](badge.png)\n\n````html\n<center><img src=\"local.png\"></center>\n<sup>Code</sup>\n````";
    assert!(special_blocks(markdown).is_empty());
}

#[test]
fn recognizes_standalone_markdown_images_and_keeps_badge_links() {
    let blocks =
        special_blocks("![Comparison](wide.png)\n\n[![Fabric](badge.png)](https://example.com)");
    let [Block::Image(comparison), Block::Image(badge)] = blocks.as_slice() else {
        panic!("expected two standalone images, got {blocks:?}");
    };
    assert_eq!(comparison.src(), "wide.png");
    assert_eq!(badge.alt(), "Fabric");
    assert_eq!(badge.link(), Some("https://example.com"));
}

fn special_blocks(source: &str) -> Vec<Block> {
    let root = to_mdast(source, &ParseOptions::gfm()).expect("parse Markdown");
    root.children()
        .expect("document blocks")
        .iter()
        .filter_map(|node| parse_block(node, source))
        .collect()
}
