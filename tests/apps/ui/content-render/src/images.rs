// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io::Cursor;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use image::codecs::gif::GifEncoder;
use image::{Delay, DynamicImage, Frame, ImageFormat, Rgba, RgbaImage};

use super::{
    MAX_DECODED_BYTES, SKELETON_HEIGHT, SKELETON_WIDTH, decode, log_target, make_skeleton,
};

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
        let mut budget = 4;
        let image = decode(&encode(1, format), &mut budget).expect("renderable image");
        assert_eq!(image.frame_count(), 1);
        assert_eq!(image.as_bytes(0), Some([30, 20, 10, 128].as_slice()));
        assert_eq!(budget, 0);
    }
}

#[test]
fn decodes_gif_and_webp_animation_frames_and_delays() {
    for (bytes, alpha) in [(animated_gif().as_slice(), 255), (ANIMATED_WEBP, 128)] {
        let mut budget = 8;
        let image = decode(bytes, &mut budget).expect("renderable animation");
        assert_eq!(image.frame_count(), 2);
        assert_eq!(image.size(0), image.size(1));
        assert_eq!(image.as_bytes(0), Some([30, 20, 10, 255].as_slice()));
        assert_eq!(image.as_bytes(1), Some([60, 50, 40, alpha].as_slice()));
        assert_eq!(Duration::from(image.delay(0)), Duration::from_millis(80));
        assert_eq!(Duration::from(image.delay(1)), Duration::from_millis(170));
        assert_eq!(budget, 0);
    }
}

#[test]
fn failed_animation_leaves_room_for_a_smaller_image() {
    let mut budget = 7;
    assert!(matches!(
        decode(&animated_gif(), &mut budget),
        Err(image::ImageError::Limits(_))
    ));
    assert_eq!(budget, 7);
    decode(&encode(1, ImageFormat::Png), &mut budget).unwrap();
    assert_eq!(budget, 3);
    assert!(matches!(
        decode(&encode(1, ImageFormat::Png), &mut budget),
        Err(image::ImageError::Limits(_))
    ));
}

#[test]
fn rejects_oversize_and_unrecognized_images() {
    let mut budget = MAX_DECODED_BYTES;
    for format in [ImageFormat::Png, ImageFormat::Gif, ImageFormat::WebP] {
        assert!(matches!(
            decode(&encode(4097, format), &mut budget),
            Err(image::ImageError::Limits(_))
        ));
    }
    assert!(matches!(
        decode(b"not an image", &mut budget),
        Err(image::ImageError::Unsupported(_))
    ));
    assert_eq!(budget, MAX_DECODED_BYTES);
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

fn animated_gif() -> Vec<u8> {
    let mut buffer = Vec::new();
    let frames = [([10, 20, 30, 255], 80), ([40, 50, 60, 255], 170)].map(|(color, delay)| {
        Frame::from_parts(
            RgbaImage::from_pixel(1, 1, Rgba(color)),
            0,
            0,
            Delay::from_saturating_duration(Duration::from_millis(delay)),
        )
    });
    GifEncoder::new(&mut buffer).encode_frames(frames).unwrap();
    buffer
}

// Two 1x1 lossless WebP frames generated with Pillow, with delays of 80 and 170 ms.
const ANIMATED_WEBP: &[u8] = b"RIFF\x88\0\0\0WEBPVP8X\x0a\0\0\0\x12\0\0\0\0\0\0\0\0\0\
    ANIM\x06\0\0\0\0\0\0\0\0\0\
    ANMF\x2a\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x50\0\0\x02\
    VP8L\x11\0\0\0\x2f\0\0\0\0\x07\x50\x8a\x2a\xd4\xa3\xff\x81\x88\xe8\x7f\0\0\
    ANMF\x2a\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\xaa\0\0\x02\
    VP8L\x11\0\0\0\x2f\0\0\0\x10\x07\x50\x99\xa2\x94\xa7\x80\x81\x88\xe8\x7f\0\0";
