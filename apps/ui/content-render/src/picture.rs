// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! A standalone image whose width and height scale together.

use std::sync::Arc;

use axlauncher_ui::kit::Size;
use axlauncher_ui::kit::base::Link;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, Button, ButtonVariants, ClickEvent, Div, IntoElement,
    MouseButton, ObjectFit, Pixels, RenderImage, Role, SharedString, Skeleton, div, img, px, size,
};

use super::images::{ImageState, SKELETON_HEIGHT, SKELETON_WIDTH};
use super::{image_placeholder, image_preview};

pub(super) fn picture(
    state: ImageState,
    width: Option<u16>,
    height: Option<u16>,
    name: SharedString,
    link: Option<SharedString>,
    cx: &App,
) -> Div {
    let intrinsic = match &state {
        ImageState::Ready(image) => image
            .size(0)
            .map(|dimension| px(i32::from(dimension) as f32)),
        ImageState::Loading | ImageState::Unavailable => {
            size(px(SKELETON_WIDTH as f32), px(SKELETON_HEIGHT as f32))
        }
    };
    let preferred = preferred_size(intrinsic, width, height);
    // Constrain the outer width before the child derives its height from the aspect ratio.
    let frame = div().w_full().max_w(preferred.width);
    if preferred.height <= Pixels::ZERO {
        return frame;
    }
    let ratio = aspect_ratio(preferred);
    match state {
        ImageState::Loading => frame.child(
            Skeleton::new()
                .w_full()
                .h_auto()
                .border_0()
                .aspect_ratio(ratio),
        ),
        ImageState::Ready(image) => frame.child(loaded_picture(image, ratio, name, link, cx)),
        ImageState::Unavailable => frame.child(unavailable_picture(ratio, name, link, cx)),
    }
}

fn loaded_picture(
    image: Arc<RenderImage>,
    ratio: f32,
    name: SharedString,
    link: Option<SharedString>,
    cx: &App,
) -> AnyElement {
    let thumbnail = img(image.clone())
        .id("picture-image")
        .absolute()
        .inset_0()
        .size_full()
        .object_fit(ObjectFit::Contain);

    match link {
        None => Button::new("open-image-preview")
            .ghost()
            .relative()
            .w_full()
            .h_auto()
            .aspect_ratio(ratio)
            .p_0()
            .accessibility_label(preview_label(&name))
            .tooltip("Open image preview")
            .on_click(move |_, window, cx| {
                image_preview::open(image.clone(), name.clone(), window, cx)
            })
            .child(thumbnail)
            .into_any_element(),
        Some(link) => picture_link(link, name, ratio, cx)
            .child(thumbnail)
            .into_any_element(),
    }
}

fn unavailable_picture(
    ratio: f32,
    name: SharedString,
    link: Option<SharedString>,
    cx: &App,
) -> AnyElement {
    let label = if name.is_empty() {
        "Image unavailable".into()
    } else {
        SharedString::from(format!("Image unavailable: {name}"))
    };
    let icon = image_placeholder::icon(cx).max_w_full().max_h_full();
    match link {
        Some(link) => picture_link(link, label, ratio, cx)
            .child(icon)
            .into_any_element(),
        None => div()
            .id("picture")
            .flex()
            .items_center()
            .justify_center()
            .w_full()
            .aspect_ratio(ratio)
            .role(Role::Image)
            .aria_label(label)
            .child(icon)
            .into_any_element(),
    }
}

fn preview_label(name: &str) -> String {
    if name.is_empty() {
        return "Open image preview".to_owned();
    }
    format!("Open image preview for {name}")
}

fn picture_link(link: SharedString, label: SharedString, ratio: f32, cx: &App) -> Link {
    let ring = cx.theme().ring;
    let auxiliary_link = link.clone();
    Link::new("picture")
        .href(link)
        .accessibility_label(label)
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .w_full()
        .aspect_ratio(ratio)
        .cursor_pointer()
        .focus_visible(move |style| style.border_1().border_color(ring))
        .open_with(|url, _, _, cx| cx.open_url(url))
        .on_aux_click(move |event, _, cx| {
            if matches!(event, ClickEvent::Mouse(click) if click.up.button == MouseButton::Middle) {
                cx.open_url(&auxiliary_link);
            }
        })
}

fn preferred_size(
    intrinsic: Size<Pixels>,
    width: Option<u16>,
    height: Option<u16>,
) -> Size<Pixels> {
    if width == Some(0) || height == Some(0) {
        return Size::default();
    }
    let width = width.map(|width| px(f32::from(width)));
    let height = height.map(|height| px(f32::from(height)));
    let ratio = aspect_ratio(intrinsic);
    match (width, height) {
        (Some(width), Some(height)) => size(width, height),
        (Some(width), None) => size(width, width / ratio),
        (None, Some(height)) => size(height * ratio, height),
        (None, None) => intrinsic,
    }
}

fn aspect_ratio(size: Size<Pixels>) -> f32 {
    if size.width <= Pixels::ZERO || size.height <= Pixels::ZERO {
        return 1.;
    }
    size.width / size.height
}

#[cfg(test)]
#[path = "../../../../tests/apps/ui/content-render/src/picture.rs"]
mod tests;
