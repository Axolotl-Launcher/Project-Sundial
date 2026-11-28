// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_ui::{Bounds, point, px, size};

use super::{ViewTransform, clamp_offset, fit_image};

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
fn a_small_image_scales_up_to_fit_the_window() {
    let intrinsic = size(px(200.), px(100.));
    assert_eq!(
        fit_image(intrinsic, size(px(600.), px(400.))),
        size(px(600.), px(300.))
    );
}

#[test]
fn cursor_anchored_zoom_keeps_the_same_image_point() {
    let intrinsic = size(px(600.), px(400.));
    let viewport = Bounds::new(point(px(10.), px(20.)), intrinsic);
    let cursor = point(px(160.), px(120.));
    let mut transform = ViewTransform::default();

    transform.zoom_at(2., cursor, intrinsic, viewport);
    let image = transform.bounds(intrinsic, viewport);
    assert_eq!(image.size, size(px(1200.), px(800.)));
    assert_eq!(cursor - image.origin, point(px(300.), px(200.)));
}

#[test]
fn panning_stops_at_the_image_edge_and_centers_an_axis_that_fits() {
    let image = size(px(1000.), px(300.));
    let viewport = size(px(600.), px(400.));
    assert_eq!(
        clamp_offset(point(px(-500.), px(75.)), image, viewport),
        point(px(-200.), px(0.))
    );
}
