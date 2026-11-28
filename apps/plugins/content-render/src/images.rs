// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Loads and decodes document images through the supplied transport.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use axlauncher_ui::{
    ActiveTheme, App, Asset, ImageSource, ImgResourceLoader, RenderImage, Resource, Rgba,
    SharedUri, Window, bounce, ease_in_out,
};
use image::{Delay, Frame, ImageReader, Limits, RgbaImage};

const PLACEHOLDER: &str = "icons/image.svg";
const MAX_IMAGE_EDGE: u32 = 4096;
pub(super) const SKELETON_WIDTH: u32 = 160;
pub(super) const SKELETON_HEIGHT: u32 = 90;
const SKELETON_CYCLE_MS: u32 = 2_000;
const SKELETON_FRAME_MS: u32 = 50;

pub(super) type ImageFetcher = fn(&str) -> Option<Vec<u8>>;

pub(super) enum ImageState {
    Loading,
    Ready(Arc<RenderImage>),
    Unavailable,
}

#[derive(Clone, Hash)]
pub(super) struct ImageRequest {
    uri: SharedUri,
    fetch: ImageFetcher,
}

impl ImageRequest {
    pub(super) fn new(uri: SharedUri, fetch: ImageFetcher) -> Self {
        Self { uri, fetch }
    }

    pub(super) fn state(&self, window: &mut Window, cx: &mut App) -> ImageState {
        match window.use_asset::<DocumentImage>(self, cx) {
            None => ImageState::Loading,
            Some(Some(image)) => ImageState::Ready(image),
            Some(None) => ImageState::Unavailable,
        }
    }

    pub(super) fn loaded(&self, cx: &mut App) -> Option<Arc<RenderImage>> {
        cx.fetch_asset::<DocumentImage>(self).flatten()
    }
}

pub(super) fn placeholder(window: &mut Window, cx: &mut App) -> Option<Arc<RenderImage>> {
    window
        .use_asset::<ImgResourceLoader>(&Resource::Embedded(PLACEHOLDER.into()), cx)
        .and_then(Result::ok)
}

pub(super) fn source(uri: &SharedUri, fetch: ImageFetcher) -> ImageSource {
    let request = ImageRequest::new(uri.clone(), fetch);
    ImageSource::Custom(Arc::new(move |window, cx| {
        match request.state(window, cx) {
            ImageState::Loading => {
                let color = Rgba::from(cx.theme().skeleton).into();
                window.use_asset::<InlineSkeleton>(&color, cx)
            }
            ImageState::Ready(image) => Some(image),
            ImageState::Unavailable => placeholder(window, cx),
        }
        .map(Ok)
    }))
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

struct DocumentImage;

impl Asset for DocumentImage {
    type Source = ImageRequest;
    type Output = Option<Arc<RenderImage>>;

    fn load(
        request: Self::Source,
        _: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        load_image(request)
    }
}

async fn load_image(request: ImageRequest) -> Option<Arc<RenderImage>> {
    let bytes = (request.fetch)(request.uri.as_ref())?;
    decode(bytes)
}

fn decode(bytes: Vec<u8>) -> Option<Arc<RenderImage>> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_EDGE);
    limits.max_image_height = Some(MAX_IMAGE_EDGE);
    reader.limits(limits);
    let mut pixels = reader.decode().ok()?.into_rgba8();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([Frame::new(pixels)])))
}

#[cfg(test)]
#[path = "../../../../tests/apps/plugins/content-render/src/images.rs"]
mod tests;
