// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! A standalone image whose width and height scale together.

use axlauncher_ui::kit::Size;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, App, Button, ButtonVariants, ClickEvent, Div, MouseButton, ObjectFit, Pixels,
    Role, SharedString, Skeleton, Stateful, div, img, px, size,
};

use super::images::{ImageState, SKELETON_HEIGHT, SKELETON_WIDTH};
use super::{image_placeholder, image_preview};

fn picture_skeleton(width: Option<u16>, height: Option<u16>) -> Div {
    let intrinsic = size(px(SKELETON_WIDTH as f32), px(SKELETON_HEIGHT as f32));
    let preferred = preferred_size(Some(intrinsic), width, height);
    div()
        .w_full()
        .max_w(preferred.width)
        .when(preferred.height > Pixels::ZERO, |frame| {
            frame.child(
                Skeleton::new()
                    .w_full()
                    .h_auto()
                    .border_0()
                    .aspect_ratio(aspect_ratio(preferred)),
            )
        })
}

pub(super) fn picture(
    state: ImageState,
    width: Option<u16>,
    height: Option<u16>,
    name: SharedString,
    link: Option<SharedString>,
    cx: &App,
) -> Div {
    let image = match state {
        ImageState::Loading => return picture_skeleton(width, height),
        ImageState::Ready(image) => image,
        ImageState::Unavailable => return unavailable_picture(width, height, name, link, cx),
    };
    let intrinsic = image
        .size(0)
        .map(|dimension| px(i32::from(dimension) as f32));
    let preferred = preferred_size(Some(intrinsic), width, height);
    let ring = cx.theme().ring;
    let thumbnail = img(image.clone())
        .absolute()
        .inset_0()
        .size_full()
        .object_fit(ObjectFit::Contain);

    // Resolve the parent's width before applying the aspect ratio. An img with
    // a max width still contributes its original explicit height to layout.
    div()
        .w_full()
        .max_w(preferred.width)
        .map(|frame| match link {
            None if preferred.height > Pixels::ZERO => frame.child(
                Button::new("open-image-preview")
                    .ghost()
                    .relative()
                    .w_full()
                    .h_auto()
                    .aspect_ratio(aspect_ratio(preferred))
                    .p_0()
                    .accessibility_label(preview_label(&name))
                    .tooltip("Open image preview")
                    .on_click(move |_, window, cx| {
                        image_preview::open(image.clone(), name.clone(), window, cx)
                    })
                    .child(thumbnail),
            ),
            link => frame.child(
                div()
                    .id("picture")
                    .relative()
                    .w_full()
                    .aspect_ratio(aspect_ratio(preferred))
                    .aria_label(name)
                    .focus_visible(move |style| style.border_1().border_color(ring))
                    .map(|element| picture_role(element, link))
                    .child(thumbnail),
            ),
        })
}

fn unavailable_picture(
    width: Option<u16>,
    height: Option<u16>,
    name: SharedString,
    link: Option<SharedString>,
    cx: &App,
) -> Div {
    let intrinsic = size(px(SKELETON_WIDTH as f32), px(SKELETON_HEIGHT as f32));
    let preferred = preferred_size(Some(intrinsic), width, height);
    let ring = cx.theme().ring;
    let label = if name.is_empty() {
        "Image unavailable".into()
    } else {
        SharedString::from(format!("Image unavailable: {name}"))
    };
    div()
        .w_full()
        .max_w(preferred.width)
        .when(preferred.height > Pixels::ZERO, |frame| {
            frame.child(
                div()
                    .id("picture")
                    .flex()
                    .items_center()
                    .justify_center()
                    .w_full()
                    .aspect_ratio(aspect_ratio(preferred))
                    .aria_label(label)
                    .focus_visible(move |style| style.border_1().border_color(ring))
                    .map(|element| picture_role(element, link))
                    .child(image_placeholder::icon(cx).max_w_full().max_h_full()),
            )
        })
}

fn preview_label(name: &str) -> String {
    if name.is_empty() {
        return "Open image preview".to_owned();
    }
    format!("Open image preview for {name}")
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
#[path = "../../../../tests/apps/ui/content-render/src/picture.rs"]
mod tests;
