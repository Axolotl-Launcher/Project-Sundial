// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! A standalone image whose width and height scale together.

use std::sync::Arc;

use axlauncher_ui::kit::Size;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, App, ClickEvent, Div, IntoElement, MouseButton, ObjectFit, Pixels, RenderImage,
    Role, SharedString, Stateful, div, img, px, size,
};

pub(super) fn picture(
    image: Option<Arc<RenderImage>>,
    width: Option<u16>,
    height: Option<u16>,
    name: SharedString,
    link: Option<SharedString>,
    cx: &App,
) -> impl IntoElement + use<> {
    let intrinsic = image.as_ref().map(|image| {
        image
            .size(0)
            .map(|dimension| px(i32::from(dimension) as f32))
    });
    let preferred = preferred_size(intrinsic, width, height);
    let ring = cx.theme().ring;

    // Resolve the parent's width before applying the aspect ratio. An img with
    // a max width still contributes its original explicit height to layout.
    div().w_full().max_w(preferred.width).child(
        div()
            .id("picture")
            .relative()
            .w_full()
            .aspect_ratio(aspect_ratio(preferred))
            .aria_label(name)
            .focus_visible(move |style| style.border_1().border_color(ring))
            .map(|element| picture_role(element, link))
            .children(image.map(|image| {
                img(image)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Contain)
            })),
    )
}

fn picture_role(element: Stateful<Div>, link: Option<SharedString>) -> Stateful<Div> {
    let Some(link) = link else {
        return element.role(Role::Image);
    };
    let auxiliary_link = link.clone();
    element
        .role(Role::Link)
        .focusable()
        .tab_stop(true)
        .cursor_pointer()
        .on_click(move |_, _, cx| cx.open_url(&link))
        .on_aux_click(move |event, _, cx| {
            if matches!(event, ClickEvent::Mouse(click) if click.up.button == MouseButton::Middle) {
                cx.open_url(&auxiliary_link);
            }
        })
}

fn preferred_size(
    intrinsic: Option<Size<Pixels>>,
    width: Option<u16>,
    height: Option<u16>,
) -> Size<Pixels> {
    if width == Some(0) || height == Some(0) {
        return Size::default();
    }
    let width = width.map(|width| px(f32::from(width)));
    let height = height.map(|height| px(f32::from(height)));
    let ratio = intrinsic.map_or(1., aspect_ratio);
    match (width, height) {
        (Some(width), Some(height)) => size(width, height),
        (Some(width), None) => size(width, width / ratio),
        (None, Some(height)) => size(height * ratio, height),
        (None, None) => intrinsic.unwrap_or_default(),
    }
}

fn aspect_ratio(size: Size<Pixels>) -> f32 {
    if size.width <= Pixels::ZERO || size.height <= Pixels::ZERO {
        return 1.;
    }
    size.width / size.height
}

#[cfg(test)]
#[path = "../../../../tests/apps/plugins/markdown/src/picture.rs"]
mod tests;
