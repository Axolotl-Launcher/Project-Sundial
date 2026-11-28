// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_ui::{Bounds, point, px, size};

use super::{ImageLayout, ViewTransform, clamp_offset, fit_image};

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
fn a_small_image_opens_at_intrinsic_size_and_fit_can_scale_it_up() {
    let intrinsic = size(px(200.), px(100.));
    let stage = Bounds::new(point(px(16.), px(16.)), size(px(600.), px(400.)));
    let layout = ImageLayout::new(intrinsic, stage, size(px(632.), px(432.)), px(16.));

    assert_eq!(ViewTransform::default().bounds(layout).size, intrinsic);
    assert_eq!(
        ViewTransform::fitted().bounds(layout).size,
        size(px(600.), px(300.))
    );
}

#[test]
fn large_images_open_with_the_previous_window_insets() {
    let window = size(px(720.), px(560.));
    let stage = Bounds::new(point(px(16.), px(16.)), size(px(688.), px(528.)));
    let wide = ImageLayout::new(size(px(1200.), px(600.)), stage, window, px(16.));
    let tall = ImageLayout::new(size(px(500.), px(1000.)), stage, window, px(16.));

    assert_eq!(
        ViewTransform::default().bounds(wide).size,
        size(px(656.), px(328.))
    );
    assert_eq!(
        ViewTransform::default().bounds(tall).size,
        size(px(216.), px(432.))
    );
}

#[test]
fn first_zoom_steps_start_at_the_visible_size_even_for_very_small_images() {
    let stage = Bounds::new(point(px(16.), px(16.)), size(px(600.), px(400.)));
    let layout = ImageLayout::new(
        size(px(20.), px(10.)),
        stage,
        size(px(632.), px(432.)),
        px(16.),
    );
    for (factor, expected) in [(0.5, px(10.)), (2., px(40.))] {
        let mut transform = ViewTransform::default();
        transform.zoom_at(factor, stage.center(), layout);
        assert!((transform.bounds(layout).size.width - expected).abs() < px(0.01));
    }
}

#[test]
fn cursor_anchored_zoom_keeps_the_same_image_point() {
    let intrinsic = size(px(600.), px(400.));
    let viewport = Bounds::new(point(px(10.), px(20.)), intrinsic);
    let layout = ImageLayout::new(intrinsic, viewport, size(px(664.), px(528.)), px(16.));
    let cursor = point(px(160.), px(120.));
    let mut transform = ViewTransform::default();

    transform.zoom_at(2., cursor, layout);
    let image = transform.bounds(layout);
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
