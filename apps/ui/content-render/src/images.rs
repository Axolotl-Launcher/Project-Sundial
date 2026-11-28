// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Loads and decodes document images through the supplied transport.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use axlauncher_ui::{ActiveTheme, App, Asset, RenderImage, Rgba, Window, bounce, ease_in_out};
use image::codecs::{gif::GifDecoder, webp::WebPDecoder};
use image::error::{DecodingError, LimitError, LimitErrorKind};
use image::{
    AnimationDecoder, Delay, DynamicImage, Frame, ImageDecoder, ImageError, ImageFormat,
    ImageReader, Limits, RgbaImage,
};
use url::Url;

const MAX_IMAGE_EDGE: u32 = 4096;
const MAX_ANIMATION_FRAMES: usize = 1024;
pub(super) const MAX_DECODED_BYTES: usize = 128 * 1024 * 1024;
pub(super) const SKELETON_WIDTH: u32 = 160;
pub(super) const SKELETON_HEIGHT: u32 = 90;
const SKELETON_CYCLE_MS: u32 = 2_000;
const SKELETON_FRAME_MS: u32 = 50;

pub(super) enum ImageState {
    Loading,
    Ready(Arc<RenderImage>),
    Unavailable,
}

pub(super) fn skeleton(window: &mut Window, cx: &mut App) -> Option<Arc<RenderImage>> {
    let color = Rgba::from(cx.theme().skeleton).into();
    window.use_asset::<InlineSkeleton>(&color, cx)
}

// TextView accepts image frames, which GPUI plays while the image request is pending.
struct InlineSkeleton;

impl Asset for InlineSkeleton {
    type Source = u32;
    type Output = Arc<RenderImage>;

    fn load(color: u32, _: &mut App) -> impl Future<Output = Self::Output> + Send + 'static {
        make_skeleton(color)
    }
}

async fn make_skeleton(color: u32) -> Arc<RenderImage> {
    let [red, green, blue, alpha] = color.to_be_bytes();
    // Match GPUI Kit's Skeleton curve and opacity range without changing the image layout.
    let ease = bounce(ease_in_out);
    let delay =
        Delay::from_saturating_duration(Duration::from_millis(u64::from(SKELETON_FRAME_MS)));
    let frames = (0..SKELETON_CYCLE_MS)
        .step_by(SKELETON_FRAME_MS as usize)
        .map(|elapsed| {
            let progress = elapsed as f32 / SKELETON_CYCLE_MS as f32;
            let opacity = 1.0 - ease(progress) * 0.5;
            let fill = image::Rgba([blue, green, red, (f32::from(alpha) * opacity).round() as u8]);
            let pixels = skeleton_pixels(fill);
            Frame::from_parts(pixels, 0, 0, delay)
        });
    Arc::new(RenderImage::new(frames.collect::<Vec<_>>()))
}

fn skeleton_pixels(fill: image::Rgba<u8>) -> RgbaImage {
    RgbaImage::from_fn(SKELETON_WIDTH, SKELETON_HEIGHT, |x, y| {
        // Windows linearly samples atlas tiles without padding. Leave a transparent texel
        // around the fill to keep adjacent animation frames from showing through at its edge.
        if x == 0 || x == SKELETON_WIDTH - 1 || y == 0 || y == SKELETON_HEIGHT - 1 {
            image::Rgba([0; 4])
        } else {
            fill
        }
    })
}

pub(super) fn report_failure(uri: &str, stage: &str, error: impl std::fmt::Display) {
    if cfg!(debug_assertions) {
        eprintln!(
            "[content-render] Image {stage} failed for {}: {error}",
            log_target(uri)
        );
    }
}

fn log_target(uri: &str) -> String {
    let Ok(mut url) = Url::parse(uri) else {
        return "<invalid URL>".to_owned();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return format!("<{} URL>", url.scheme());
    }
    url.set_query(None);
    url.set_fragment(None);
    let _ = url.set_password(None);
    let _ = url.set_username("");
    url.into()
}

pub(super) fn decode(
    bytes: &[u8],
    document_bytes: &mut usize,
) -> image::ImageResult<Arc<RenderImage>> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_EDGE);
    limits.max_image_height = Some(MAX_IMAGE_EDGE);
    // Commit the budget only after the complete image has decoded successfully.
    let mut remaining = *document_bytes;
    let frames = match reader.format() {
        Some(ImageFormat::Gif) => {
            let mut decoder = GifDecoder::new(Cursor::new(bytes))?;
            decoder.set_limits(limits)?;
            decode_animation(decoder, ImageFormat::Gif, &mut remaining)?
        }
        Some(ImageFormat::WebP) => {
            let mut decoder = WebPDecoder::new(Cursor::new(bytes))?;
            if decoder.has_animation() {
                decoder.set_limits(limits)?;
                decode_animation(decoder, ImageFormat::WebP, &mut remaining)?
            } else {
                vec![decode_static(decoder, limits, &mut remaining)?]
            }
        }
        _ => {
            reader.limits(limits.clone());
            vec![decode_static(
                reader.into_decoder()?,
                limits,
                &mut remaining,
            )?]
        }
    };
    *document_bytes = remaining;
    Ok(Arc::new(RenderImage::new(frames)))
}

fn decode_static(
    mut decoder: impl ImageDecoder,
    mut limits: Limits,
    remaining: &mut usize,
) -> image::ImageResult<Frame> {
    let (width, height) = decoder.dimensions();
    limits.check_dimensions(width, height)?;
    reserve_bytes(remaining, width as usize * height as usize * 4)?;
    limits.reserve(decoder.total_bytes())?;
    decoder.set_limits(limits)?;
    let pixels = DynamicImage::from_decoder(decoder)?.into_rgba8();
    Ok(bgra_frame(Frame::new(pixels)))
}

fn decode_animation<'a>(
    decoder: impl AnimationDecoder<'a>,
    format: ImageFormat,
    remaining: &mut usize,
) -> image::ImageResult<Vec<Frame>> {
    let mut frames = Vec::new();
    for frame in decoder.into_frames() {
        let frame = frame?;
        if frames.len() == MAX_ANIMATION_FRAMES {
            return Err(decoded_limit());
        }
        reserve_bytes(remaining, frame.buffer().len())?;
        frames.push(bgra_frame(frame));
    }
    if frames.is_empty() {
        return Err(ImageError::Decoding(DecodingError::new(
            format.into(),
            "Image contains no frames",
        )));
    }
    Ok(frames)
}

fn reserve_bytes(remaining: &mut usize, bytes: usize) -> image::ImageResult<()> {
    *remaining = remaining.checked_sub(bytes).ok_or_else(decoded_limit)?;
    Ok(())
}

fn decoded_limit() -> ImageError {
    ImageError::Limits(LimitError::from_kind(LimitErrorKind::InsufficientMemory))
}

fn bgra_frame(mut frame: Frame) -> Frame {
    for pixel in frame.buffer_mut().as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    frame
}

#[cfg(test)]
#[path = "../../../../tests/apps/ui/content-render/src/images.rs"]
mod tests;
