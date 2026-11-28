// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! A modal view of a decoded Markdown image.

use std::sync::Arc;

use axlauncher_ui::kit::Size;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, App, Button, ButtonVariants, DialogEntrance, IconName, IntoElement, ObjectFit,
    Pixels, RenderImage, Role, SharedString, Sizable, Window, WindowExt, div, h_flex, img, size,
    v_flex,
};

const HORIZONTAL_INSET_REMS: f32 = 4.;
const VERTICAL_INSET_REMS: f32 = 8.;
const MIN_VIEWER_REMS: f32 = 12.;

pub(super) fn open(image: Arc<RenderImage>, name: SharedString, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let viewport = window.viewport_size();
        let rem = window.rem_size();
        let intrinsic = image
            .size(0)
            .map(|pixels| Pixels::from(i32::from(pixels) as f32));
        let available = size(
            viewport.width - rem * HORIZONTAL_INSET_REMS,
            viewport.height - rem * VERTICAL_INSET_REMS,
        );
        let fitted = fit_image(intrinsic, available);
        dialog
            .centered()
            .entrance(DialogEntrance::Fade)
            .close_button(false)
            .min_h_0()
            .p_0()
            .border_0()
            .shadow_none()
            .bg(cx.theme().background.opacity(0.))
            .w(fitted.width.max(rem * MIN_VIEWER_REMS))
            .child(viewer(image.clone(), &name, fitted, cx))
    });
}

fn viewer(
    image: Arc<RenderImage>,
    name: &str,
    fitted: Size<Pixels>,
    cx: &App,
) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let caption = name.trim();
    let accessible_name = if caption.is_empty() {
        "Image preview"
    } else {
        caption
    };
    v_flex()
        .w_full()
        .items_center()
        .gap_2()
        .child(
            h_flex().w_full().justify_end().child(
                Button::new("close-image-preview")
                    .small()
                    .ghost()
                    .icon(IconName::Close)
                    .rounded(theme.radius_full())
                    .bg(theme.popover)
                    .text_color(theme.popover_foreground)
                    .accessibility_label("Close image preview")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            ),
        )
        .child(
            div()
                .id("markdown-image-preview")
                .role(Role::Image)
                .aria_label(accessible_name.to_owned())
                .w(fitted.width)
                .max_w_full()
                .h(fitted.height)
                .child(img(image).size_full().object_fit(ObjectFit::Contain)),
        )
        .when(!caption.is_empty(), |viewer| {
            viewer.child(
                div()
                    .max_w_full()
                    .truncate()
                    .rounded(theme.radius)
                    .bg(theme.popover)
                    .text_color(theme.popover_foreground)
                    .text_sm()
                    .px_2()
                    .py_1()
                    .child(caption.to_owned()),
            )
        })
}

fn fit_image(intrinsic: Size<Pixels>, available: Size<Pixels>) -> Size<Pixels> {
    let scale = (available.width / intrinsic.width)
        .min(available.height / intrinsic.height)
        .clamp(0., 1.);
    size(intrinsic.width * scale, intrinsic.height * scale)
}

#[cfg(test)]
#[path = "../../../../tests/apps/plugins/markdown/src/image_preview.rs"]
mod tests;
