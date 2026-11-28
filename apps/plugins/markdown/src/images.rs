// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Loads and decodes Markdown images through the supplied transport.

use std::io::Cursor;
use std::sync::Arc;

use axlauncher_ui::{App, Asset, ImageSource, ImgResourceLoader, RenderImage, Resource, SharedUri};
use image::{Frame, ImageReader, Limits};

const PLACEHOLDER: &str = "icons/image.svg";
const MAX_IMAGE_EDGE: u32 = 4096;

pub(super) type ImageFetcher = fn(&str) -> Option<Vec<u8>>;

#[derive(Clone, Hash)]
pub(super) struct ImageRequest {
    uri: SharedUri,
    fetch: ImageFetcher,
}

impl ImageRequest {
    pub(super) fn new(uri: SharedUri, fetch: ImageFetcher) -> Self {
        Self { uri, fetch }
    }

    pub(super) fn loaded(
        &self,
        window: &mut axlauncher_ui::Window,
        cx: &mut App,
    ) -> Option<Arc<RenderImage>> {
        window.use_asset::<MarkdownImage>(self, cx).flatten()
    }
}

pub(super) fn placeholder(
    window: &mut axlauncher_ui::Window,
    cx: &mut App,
) -> Option<Arc<RenderImage>> {
    window
        .use_asset::<ImgResourceLoader>(&Resource::Embedded(PLACEHOLDER.into()), cx)
        .and_then(Result::ok)
}

pub(super) fn source(uri: &SharedUri, fetch: ImageFetcher) -> ImageSource {
    let request = ImageRequest::new(uri.clone(), fetch);
    ImageSource::Custom(Arc::new(move |window, cx| {
        request
            .loaded(window, cx)
            .or_else(|| placeholder(window, cx))
            .map(Ok)
    }))
}

struct MarkdownImage;

impl Asset for MarkdownImage {
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
#[path = "../../../../tests/apps/plugins/markdown/src/images.rs"]
mod tests;
