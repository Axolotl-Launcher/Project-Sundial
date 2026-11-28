// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_ui::{px, size};

use super::{aspect_ratio, preferred_size};

#[test]
fn uses_the_intrinsic_dimensions_and_aspect_ratio() {
    let dimensions = preferred_size(size(px(800.), px(400.)), None, None);
    assert_eq!(dimensions, size(px(800.), px(400.)));
    assert_eq!(aspect_ratio(dimensions), 2.);
}

#[test]
fn respects_dimensions_declared_in_html() {
    let dimensions = preferred_size(size(px(800.), px(400.)), Some(400), Some(100));
    assert_eq!(dimensions, size(px(400.), px(100.)));
    assert_eq!(aspect_ratio(dimensions), 4.);
    let badge = preferred_size(size(px(800.), px(400.)), None, Some(42));
    assert_eq!(badge, size(px(84.), px(42.)));
}

#[test]
fn zero_html_dimensions_leave_an_empty_image_box() {
    let intrinsic = size(px(800.), px(400.));
    assert_eq!(
        preferred_size(intrinsic, Some(400), Some(0)),
        size(px(0.), px(0.))
    );
    assert_eq!(
        preferred_size(intrinsic, Some(0), Some(200)),
        size(px(0.), px(0.))
    );
}
