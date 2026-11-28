// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::sync::Arc;

use axlauncher_ui::SvgRenderer;

use super::inline_svg;

#[test]
fn inline_placeholder_has_a_small_visible_glyph_inside_a_transparent_slot() {
    let image = SvgRenderer::new(Arc::new(()))
        .render_single_frame(inline_svg(0x345678ff).as_bytes(), 1.)
        .expect("render the inline SVG");
    let edge = i32::from(image.size(0).width) as usize;
    let (pixels, _) = image.as_bytes(0).expect("has a frame").as_chunks::<4>();
    assert!(pixels.iter().any(|pixel| pixel[3] > 0));
    for (index, pixel) in pixels.iter().enumerate() {
        if index % edge < edge / 4 || index % edge >= edge * 3 / 4 {
            assert_eq!(pixel[3], 0);
        }
    }
}
