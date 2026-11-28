// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! A bounded cache whose image requests and textures belong to one document.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_lock::{Mutex, Semaphore};
use axlauncher_ui::{
    App, AppContext as _, Asset, BackgroundExecutor, Entity, EntityId, ImageSource, RenderImage,
    SharedUri, Window,
};

use super::image_placeholder;
use super::images::{ImageState, MAX_DECODED_BYTES, decode, report_failure, skeleton};

const MAX_IMAGES: usize = 128;
const MAX_ENCODED_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMAGE_LOADS: usize = 8;
const MAX_IMAGE_DECODES: usize = 2;
static IMAGE_LOADS: Semaphore = Semaphore::new(MAX_IMAGE_LOADS);
static IMAGE_DECODES: Semaphore = Semaphore::new(MAX_IMAGE_DECODES);

type ImageDownload = Pin<Box<dyn Future<Output = Result<Vec<u8>, ImageFetchError>> + Send>>;
pub(super) type ImageFetcher = Arc<dyn Fn(String) -> ImageDownload + Send + Sync>;

/// An image transport failure with an optional minimum delay before a manual retry.
#[derive(Debug)]
pub struct ImageFetchError {
    detail: String,
    retry_delay: Option<Duration>,
}

impl ImageFetchError {
    /// Records a transport error that may be retried after `delay`.
    pub fn retryable(detail: impl Into<String>, delay: Duration) -> Self {
        Self {
            detail: detail.into(),
            retry_delay: Some(delay),
        }
    }

    /// Records a transport error that is ineligible for manual retry.
    pub fn permanent(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            retry_delay: None,
        }
    }
}

#[derive(Clone)]
enum ImageFailure {
    RetryAfter { failed_at: Instant, delay: Duration },
    DownloadRejected,
    EncodedLimit,
    Decode,
    DecodeLimit,
}

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
    remaining_bytes: Arc<Mutex<usize>>,
}

impl ImageCache {
    pub(super) fn new(fetch: ImageFetcher, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            cx.on_release(Self::release).detach();
            Self {
                document: cx.entity_id(),
                fetch,
                requests: HashMap::new(),
                remaining_bytes: Arc::new(Mutex::new(MAX_DECODED_BYTES)),
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
            Some(Ok(image)) => ImageState::Ready(image),
            Some(Err(_)) => ImageState::Unavailable,
        }
    }

    pub(super) fn loaded(
        cache: &Entity<Self>,
        uri: SharedUri,
        cx: &mut App,
    ) -> Option<Arc<RenderImage>> {
        let request = cache.read(cx).requests.get(&uri)?.clone();
        cx.fetch_asset::<DocumentImage>(&request)?.ok()
    }

    pub(super) fn retry(&mut self, cx: &mut App) -> bool {
        let now = cx.background_executor().now();
        let previous_count = self.requests.len();
        self.requests
            .retain(|_, request| !request.evict_for_retry(now, cx));
        self.requests.len() != previous_count
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
            fetch: self.fetch.clone(),
            remaining_bytes: self.remaining_bytes.clone(),
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
    remaining_bytes: Arc<Mutex<usize>>,
}

impl ImageRequest {
    fn evict_for_retry(&self, now: Instant, cx: &mut App) -> bool {
        let Some(Err(ImageFailure::RetryAfter { failed_at, delay })) =
            cx.fetch_asset::<DocumentImage>(self)
        else {
            return false;
        };
        if now.saturating_duration_since(failed_at) < delay {
            return false;
        }
        cx.remove_asset::<DocumentImage>(self);
        true
    }

    fn release(&self, cx: &mut App) {
        if let Some(Ok(image)) = cx.fetch_asset::<DocumentImage>(self) {
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
    type Output = Result<Arc<RenderImage>, ImageFailure>;

    fn load(
        request: ImageRequest,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        load(request, cx.background_executor().clone())
    }
}

async fn load(
    request: ImageRequest,
    executor: BackgroundExecutor,
) -> Result<Arc<RenderImage>, ImageFailure> {
    let uri = request.uri.as_ref();
    // Hold the load slot through decode so downloaded responses cannot pile up.
    let _loading = IMAGE_LOADS.acquire().await;
    let bytes = (request.fetch)(uri.to_owned())
        .await
        .map_err(|error| download_failure(uri, error, executor.now()))?;
    if bytes.len() > MAX_ENCODED_IMAGE_BYTES {
        report_failure(
            uri,
            "download (limit)",
            format_args!("image response exceeds {MAX_ENCODED_IMAGE_BYTES} bytes"),
        );
        return Err(ImageFailure::EncodedLimit);
    }
    let bytes = bytes.into_boxed_slice();
    let mut remaining = request.remaining_bytes.lock().await;
    let _decoding = IMAGE_DECODES.acquire().await;
    decode(&bytes, &mut remaining).map_err(|error| decode_failure(uri, &error))
}

fn download_failure(uri: &str, error: ImageFetchError, failed_at: Instant) -> ImageFailure {
    match error.retry_delay {
        Some(delay) => {
            report_failure(uri, "download (retryable)", error.detail);
            ImageFailure::RetryAfter { failed_at, delay }
        }
        None => {
            report_failure(uri, "download (permanent)", error.detail);
            ImageFailure::DownloadRejected
        }
    }
}

fn decode_failure(uri: &str, error: &image::ImageError) -> ImageFailure {
    if matches!(error, image::ImageError::Limits(_)) {
        report_failure(uri, "decode (limit)", error);
        return ImageFailure::DecodeLimit;
    }
    report_failure(uri, "decode", error);
    ImageFailure::Decode
}

#[cfg(test)]
#[path = "../../../../tests/apps/ui/content-render/src/image_cache.rs"]
mod tests;
