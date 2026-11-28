// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! A bounded cache whose image requests and textures belong to one document.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axlauncher_ui::{
    App, AppContext as _, Asset, Entity, EntityId, ImageSource, RenderImage, SharedUri, Window,
};

use super::image_placeholder;
use super::images::{ImageFetcher, ImageState, load_image, skeleton};

const MAX_IMAGES: usize = 128;
const MAX_DECODED_BYTES: usize = 128 * 1024 * 1024;

pub(super) fn source(uri: Option<SharedUri>, cache: Entity<ImageCache>) -> ImageSource {
    ImageSource::Custom(Arc::new(move |window, cx| {
        let state = uri.clone().map_or(ImageState::Unavailable, |uri| {
            ImageCache::state(&cache, uri, window, cx)
        });
        match state {
            ImageState::Loading => skeleton(window, cx),
            ImageState::Ready(image) => Some(image),
            ImageState::Unavailable => image_placeholder::inline(window, cx),
        }
        .map(Ok)
    }))
}

pub(super) struct ImageCache {
    document: EntityId,
    fetch: ImageFetcher,
    requests: HashMap<SharedUri, ImageRequest>,
    decoded_bytes: Arc<AtomicUsize>,
}

impl ImageCache {
    pub(super) fn new(fetch: ImageFetcher, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            cx.on_release(Self::release).detach();
            Self {
                document: cx.entity_id(),
                fetch,
                requests: HashMap::new(),
                decoded_bytes: Arc::default(),
            }
        })
    }

    pub(super) fn state(
        cache: &Entity<Self>,
        uri: SharedUri,
        window: &mut Window,
        cx: &mut App,
    ) -> ImageState {
        let request = cache.update(cx, |cache, _| cache.admit(uri));
        let Some(request) = request else {
            return ImageState::Unavailable;
        };
        match window.use_asset::<DocumentImage>(&request, cx) {
            None => ImageState::Loading,
            Some(Some(image)) => ImageState::Ready(image),
            Some(None) => ImageState::Unavailable,
        }
    }

    pub(super) fn loaded(
        cache: &Entity<Self>,
        uri: SharedUri,
        cx: &mut App,
    ) -> Option<Arc<RenderImage>> {
        let request = cache.read(cx).requests.get(&uri)?.clone();
        cx.fetch_asset::<DocumentImage>(&request).flatten()
    }

    pub(super) fn retry(&mut self, cx: &mut App) {
        self.requests
            .retain(|_, request| request.retain_for_retry(cx));
    }

    fn admit(&mut self, uri: SharedUri) -> Option<ImageRequest> {
        if let Some(request) = self.requests.get(&uri) {
            return Some(request.clone());
        }
        if self.requests.len() == MAX_IMAGES {
            return None;
        }
        let request = ImageRequest {
            document: self.document,
            uri: uri.clone(),
            fetch: self.fetch,
            decoded_bytes: self.decoded_bytes.clone(),
        };
        self.requests.insert(uri, request.clone());
        Some(request)
    }

    fn release(&mut self, cx: &mut App) {
        for request in self.requests.values() {
            request.release(cx);
        }
    }
}

#[derive(Clone)]
struct ImageRequest {
    document: EntityId,
    uri: SharedUri,
    fetch: ImageFetcher,
    decoded_bytes: Arc<AtomicUsize>,
}

impl ImageRequest {
    fn retain_for_retry(&self, cx: &mut App) -> bool {
        if matches!(cx.fetch_asset::<DocumentImage>(self), Some(None)) {
            cx.remove_asset::<DocumentImage>(self);
            return false;
        }
        true
    }

    fn release(&self, cx: &mut App) {
        if let Some(Some(image)) = cx.fetch_asset::<DocumentImage>(self) {
            cx.drop_image(image, None);
        }
        cx.remove_asset::<DocumentImage>(self);
    }
}

impl Hash for ImageRequest {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.document.hash(state);
        self.uri.hash(state);
    }
}

struct DocumentImage;

impl Asset for DocumentImage {
    type Source = ImageRequest;
    type Output = Option<Arc<RenderImage>>;

    fn load(
        request: ImageRequest,
        _: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        load(request)
    }
}

async fn load(request: ImageRequest) -> Option<Arc<RenderImage>> {
    let image = load_image(request.uri.as_ref(), request.fetch)?;
    let bytes = image.as_bytes(0)?.len();
    reserve_bytes(&request.decoded_bytes, bytes).then_some(image)
}

fn reserve_bytes(used: &AtomicUsize, bytes: usize) -> bool {
    used.try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
        used.checked_add(bytes)
            .filter(|total| *total <= MAX_DECODED_BYTES)
    })
    .is_ok()
}

#[cfg(test)]
#[path = "../../../../tests/apps/ui/content-render/src/image_cache.rs"]
mod tests;
