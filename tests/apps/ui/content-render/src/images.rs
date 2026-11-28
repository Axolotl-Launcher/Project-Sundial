// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io::Cursor;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

use super::{SKELETON_HEIGHT, SKELETON_WIDTH, decode, log_target, make_skeleton};

#[test]
fn skeleton_pulses_without_varying_size_or_hue() {
    let Poll::Ready(image) =
        pin!(make_skeleton(0x0a141ec8)).poll(&mut Context::from_waker(Waker::noop()))
    else {
        panic!("the skeleton has no async dependencies");
    };
    let frame_count = image.frame_count();
    assert!(frame_count >= 3);
    let width = SKELETON_WIDTH as usize;
    let height = SKELETON_HEIGHT as usize;
    let interior = (width + 1) * 4;
    for index in 0..frame_count {
        assert_eq!(image.size(index), image.size(0));
        let pixels = image.as_bytes(index).unwrap();
        assert_eq!(&pixels[interior..interior + 3], &[30, 20, 10]);
        assert!(pixels[..width * 4].iter().all(|byte| *byte == 0));
        assert!(
            pixels[(height - 1) * width * 4..]
                .iter()
                .all(|byte| *byte == 0)
        );
        assert!(
            pixels
                .chunks_exact(width * 4)
                .all(|row| { row[..4] == [0; 4] && row[row.len() - 4..] == [0; 4] })
        );
    }
    let alpha = |index| image.as_bytes(index).unwrap()[interior + 3];
    assert_eq!(alpha(0), 200);
    assert_eq!(alpha(frame_count / 2), 100);
    assert_eq!(alpha(frame_count - 1), alpha(1));
    let cycle: Duration = (0..frame_count)
        .map(|index| Duration::from(image.delay(index)))
        .sum();
    assert_eq!(cycle, Duration::from_secs(2));
}

#[test]
fn decodes_png_and_webp_from_content_and_preserves_alpha() {
    for format in [ImageFormat::Png, ImageFormat::WebP] {
        let image = decode(encode(1, format)).expect("renderable image");
        assert_eq!(image.as_bytes(0), Some([30, 20, 10, 128].as_slice()));
    }
}

#[test]
fn rejects_oversize_and_unrecognized_images() {
    assert!(matches!(
        decode(encode(4097, ImageFormat::Png)),
        Err(image::ImageError::Limits(_))
    ));
    assert!(matches!(
        decode(b"not an image".to_vec()),
        Err(image::ImageError::Unsupported(_))
    ));
}

#[test]
fn diagnostic_urls_omit_credentials_and_query_parameters() {
    assert_eq!(
        log_target("https://user:secret@example.com/image.png?token=secret#preview"),
        "https://example.com/image.png"
    );
    assert_eq!(log_target("data:image/png;base64,embedded"), "<data URL>");
}

fn encode(width: u32, format: ImageFormat) -> Vec<u8> {
    let pixels = RgbaImage::from_pixel(width, 1, Rgba([10, 20, 30, 128]));
    let mut buffer = Cursor::new(Vec::new());
    DynamicImage::from(pixels)
        .write_to(&mut buffer, format)
        .expect("encode test image");
    buffer.into_inner()
}
