// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The failed-image indicator for standalone and inline images.

use std::sync::Arc;

use axlauncher_ui::{ActiveTheme, App, Asset, RenderImage, Rgba, Styled as _, Svg, Window, svg};

const ICON: &str = include_str!("../assets/image-unavailable.svg");
const INLINE_SIZE: u32 = 160;
const ICON_SIZE: u32 = 24;

pub(super) fn icon(cx: &App) -> Svg {
    svg()
        .data(ICON.as_bytes())
        .size_6()
        .text_color(cx.theme().muted_foreground)
}

pub(super) fn inline(window: &mut Window, cx: &mut App) -> Option<Arc<RenderImage>> {
    let color = Rgba::from(cx.theme().muted_foreground).into();
    window.use_asset::<InlinePlaceholder>(&color, cx).flatten()
}

struct InlinePlaceholder;

impl Asset for InlinePlaceholder {
    type Source = u32;
    type Output = Option<Arc<RenderImage>>;

    fn load(color: u32, cx: &mut App) -> impl Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move {
            renderer
                .render_single_frame(inline_svg(color).as_bytes(), 1.)
                .ok()
        }
    }
}

fn inline_svg(color: u32) -> String {
    // TextView accepts an image instead of a child element. Keep the glyph small inside its slot.
    let inset = (INLINE_SIZE - ICON_SIZE) / 2;
    let icon = ICON.replace("currentColor", &format!("#{color:08x}"));
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{INLINE_SIZE}" height="{INLINE_SIZE}"><g transform="translate({inset} {inset})">{icon}</g></svg>"#
    )
}

#[cfg(test)]
#[path = "../../../../tests/apps/ui/content-render/src/image_placeholder.rs"]
mod tests;
