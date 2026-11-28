// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

use super::decode;

#[test]
fn decodes_png_and_webp_from_content_and_preserves_alpha() {
    for format in [ImageFormat::Png, ImageFormat::WebP] {
        let image = decode(encode(1, format)).expect("renderable image");
        assert_eq!(image.as_bytes(0), Some([30, 20, 10, 128].as_slice()));
    }
}

#[test]
fn rejects_oversize_and_unrecognized_images() {
    assert!(decode(encode(4097, ImageFormat::Png)).is_none());
    assert!(decode(b"not an image".to_vec()).is_none());
}

fn encode(width: u32, format: ImageFormat) -> Vec<u8> {
    let pixels = RgbaImage::from_pixel(width, 1, Rgba([10, 20, 30, 128]));
    let mut buffer = Cursor::new(Vec::new());
    DynamicImage::from(pixels)
        .write_to(&mut buffer, format)
        .expect("encode test image");
    buffer.into_inner()
}
