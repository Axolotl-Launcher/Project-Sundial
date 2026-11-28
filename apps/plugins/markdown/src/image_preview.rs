// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! A modal view of a decoded Markdown image.

use std::sync::Arc;

use axlauncher_ui::kit::Size;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    App, DialogEntrance, ObjectFit, Pixels, RenderImage, Role, SharedString, Window, WindowExt,
    div, img, size,
};

const VIEWPORT_WIDTH: f32 = 0.9;
const VIEWPORT_HEIGHT: f32 = 0.8;
const MIN_DIALOG_REMS: f32 = 16.;

pub(super) fn open(image: Arc<RenderImage>, name: SharedString, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, _| {
        let viewport = window.viewport_size();
        let rem = window.rem_size();
        let intrinsic = image
            .size(0)
            .map(|pixels| Pixels::from(i32::from(pixels) as f32));
        let available = size(
            viewport.width * VIEWPORT_WIDTH,
            viewport.height * VIEWPORT_HEIGHT,
        );
        let fitted = fit_image(intrinsic, available);
        dialog
            .centered()
            .entrance(DialogEntrance::None)
            .title("Image preview")
            .w((fitted.width + rem * 2.).max(rem * MIN_DIALOG_REMS))
            .child(
                div()
                    .id("markdown-image-preview")
                    .role(Role::Image)
                    .aria_label(name.clone())
                    .w(fitted.width)
                    .max_w_full()
                    .mx_auto()
                    .h(fitted.height)
                    .child(
                        img(image.clone())
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                    ),
            )
    });
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
