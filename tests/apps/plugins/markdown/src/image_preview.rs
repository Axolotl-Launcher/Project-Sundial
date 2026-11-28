// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_ui::{px, size};

use super::fit_image;

#[test]
fn wide_and_tall_images_fit_without_cropping() {
    let available = size(px(600.), px(400.));
    assert_eq!(
        fit_image(size(px(1200.), px(600.)), available),
        size(px(600.), px(300.))
    );
    assert_eq!(
        fit_image(size(px(500.), px(1000.)), available),
        size(px(200.), px(400.))
    );
}

#[test]
fn an_image_that_fits_uses_its_intrinsic_size() {
    let intrinsic = size(px(200.), px(100.));
    assert_eq!(fit_image(intrinsic, size(px(600.), px(400.))), intrinsic);
}
