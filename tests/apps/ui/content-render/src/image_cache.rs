// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{future::pending, time::Duration};

use async_lock::Semaphore;
use axlauncher_ui::{App, Entity};
use gpui::TestAppContext;
use image::codecs::gif::GifEncoder;
use image::{Frame, RgbaImage};

use super::{
    DocumentImage, ImageCache, ImageFailure, ImageFetchError, ImageFetcher, ImageRequest,
    MAX_DECODED_BYTES, MAX_ENCODED_IMAGE_BYTES,
};

#[test]
fn distinct_images_in_a_document_start_before_either_download_finishes() {
    let cx = TestAppContext::single();
    let started = Arc::new(AtomicUsize::new(0));
    let finish = Arc::new(Semaphore::new(0));
    let fetch_started = started.clone();
    let fetch_finish = finish.clone();
    let fetch: ImageFetcher = Arc::new(move |_| {
        let started = fetch_started.clone();
        let finish = fetch_finish.clone();
        Box::pin(async move {
            started.fetch_add(1, Ordering::Relaxed);
            let _permission = finish.acquire().await;
            Ok(animated_image())
        })
    });
    let (document, requests) = cx.update(|cx| {
        let document = ImageCache::new(fetch, cx);
        let requests = ["one.gif", "two.gif"].map(|path| queue_image(&document, path, cx));
        queue_image(&document, "one.gif", cx);
        (document, requests)
    });
    cx.run_until_parked();
    assert_eq!(started.load(Ordering::Relaxed), 2);
    finish.add_permits(2);
    cx.run_until_parked();
    cx.update(|cx| {
        for request in &requests {
            assert!(matches!(
                cx.fetch_asset::<DocumentImage>(request),
                Some(Ok(_))
            ));
        }
        assert_eq!(
            *document.read(cx).remaining_bytes.try_lock().unwrap(),
            MAX_DECODED_BYTES - 16,
        );
    });
    cx.quit();
}

#[test]
fn oversized_custom_response_is_rejected_before_decode() {
    let cx = TestAppContext::single();
    let fetch: ImageFetcher = Arc::new(|_| {
        Box::pin(async {
            let mut bytes = animated_image();
            bytes.resize(MAX_ENCODED_IMAGE_BYTES + 1, 0);
            Ok(bytes)
        })
    });
    let (document, request) = cx.update(|cx| {
        let document = ImageCache::new(fetch, cx);
        let request = queue_image(&document, "oversized.gif", cx);
        (document, request)
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(matches!(
            cx.fetch_asset::<DocumentImage>(&request),
            Some(Err(ImageFailure::EncodedLimit))
        ));
        assert!(!document.update(cx, |cache, cx| cache.retry(cx)));
        assert_eq!(
            *document.read(cx).remaining_bytes.try_lock().unwrap(),
            MAX_DECODED_BYTES
        );
    });
    drop(document);
    cx.quit();
}

#[test]
fn releasing_one_document_evicts_only_its_image_assets() {
    let cx = TestAppContext::single();
    let fetch: ImageFetcher = Arc::new(|_| Box::pin(async { Ok(animated_image()) }));
    let (document, other, request, other_request) = cx.update(|cx| {
        let document = ImageCache::new(fetch.clone(), cx);
        let other = ImageCache::new(fetch, cx);
        let request = document
            .update(cx, |cache, _| {
                cache.admit("https://example.com/picture.png".into())
            })
            .unwrap();
        let other_request = other
            .update(cx, |cache, _| {
                cache.admit("https://example.com/picture.png".into())
            })
            .unwrap();
        let _ = cx.fetch_asset::<DocumentImage>(&request);
        let _ = cx.fetch_asset::<DocumentImage>(&other_request);
        (document, other, request, other_request)
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(matches!(
            cx.fetch_asset::<DocumentImage>(&request),
            Some(Ok(_))
        ));
        assert!(matches!(
            cx.fetch_asset::<DocumentImage>(&other_request),
            Some(Ok(_))
        ));
        for cache in [&document, &other] {
            assert_eq!(
                *cache.read(cx).remaining_bytes.try_lock().unwrap(),
                MAX_DECODED_BYTES - 8,
            );
        }
    });
    drop(document);
    cx.update(|_| {});
    cx.update(|cx| {
        assert!(!cx.has_asset::<DocumentImage>(&request));
        assert!(cx.has_asset::<DocumentImage>(&other_request));
    });
    drop(other);
    cx.quit();
}

#[test]
fn releasing_a_document_cancels_a_pending_image_asset() {
    let cx = TestAppContext::single();
    let loading = Arc::new(Semaphore::new(1));
    let fetch_loading = loading.clone();
    let fetch: ImageFetcher = Arc::new(move |_| Box::pin(hold_image(fetch_loading.clone())));
    let (document, request) = cx.update(|cx| {
        let document = ImageCache::new(fetch, cx);
        let request = document.update(cx, |cache, _| {
            cache
                .admit("https://example.com/pending.gif".into())
                .unwrap()
        });
        assert!(cx.fetch_asset::<DocumentImage>(&request).is_none());
        (document, request)
    });
    cx.run_until_parked();
    assert!(loading.try_acquire().is_none());
    drop(document);
    cx.update(|_| {});
    cx.run_until_parked();
    cx.update(|cx| assert!(!cx.has_asset::<DocumentImage>(&request)));
    assert!(loading.try_acquire().is_some());
    cx.quit();
}

async fn hold_image(loading: Arc<Semaphore>) -> Result<Vec<u8>, ImageFetchError> {
    let _permit = loading.acquire().await;
    pending().await
}

#[test]
fn retry_respects_server_delay_and_keeps_other_image_results() {
    let cx = TestAppContext::single();
    let fetch: ImageFetcher = Arc::new(|url| Box::pin(retry_fixture(url)));
    let (document, requests) = cx.update(|cx| {
        let document = ImageCache::new(fetch, cx);
        let requests = ["wait", "offline", "rejected", "invalid", "ready", "pending"]
            .map(|path| queue_image(&document, path, cx));
        (document, requests)
    });
    cx.run_until_parked();
    let [delayed, offline, rejected, invalid, ready, loading] = requests;
    cx.update(|cx| {
        assert!(matches!(
            cx.fetch_asset::<DocumentImage>(&invalid),
            Some(Err(ImageFailure::Decode))
        ));
        assert!(document.update(cx, |cache, cx| cache.retry(cx)));
        assert!(!cx.has_asset::<DocumentImage>(&offline));
        assert!(cx.has_asset::<DocumentImage>(&delayed));
        assert!(!document.update(cx, |cache, cx| cache.retry(cx)));
    });
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.update(|cx| {
        assert!(document.update(cx, |cache, cx| cache.retry(cx)));
        assert!(!cx.has_asset::<DocumentImage>(&delayed));
        for request in [rejected, invalid, ready, loading] {
            assert!(cx.has_asset::<DocumentImage>(&request));
        }
    });
    drop(document);
    cx.quit();
}

fn queue_image(document: &Entity<ImageCache>, path: &str, cx: &mut App) -> ImageRequest {
    let uri = format!("https://example.com/{path}").into();
    let request = document.update(cx, |cache, _| cache.admit(uri).unwrap());
    assert!(cx.fetch_asset::<DocumentImage>(&request).is_none());
    request
}

async fn retry_fixture(url: String) -> Result<Vec<u8>, ImageFetchError> {
    match url.rsplit('/').next().unwrap() {
        "wait" => Err(ImageFetchError::retryable(
            "HTTP 429",
            Duration::from_secs(5),
        )),
        "offline" => Err(ImageFetchError::retryable("offline", Duration::ZERO)),
        "rejected" => Err(ImageFetchError::permanent("HTTP 404")),
        "invalid" => Ok(Vec::new()),
        "ready" => Ok(animated_image()),
        "pending" => pending().await,
        path => panic!("unexpected path {path}"),
    }
}

fn animated_image() -> Vec<u8> {
    let mut bytes = Vec::new();
    let frames = [
        Frame::new(RgbaImage::new(1, 1)),
        Frame::new(RgbaImage::new(1, 1)),
    ];
    GifEncoder::new(&mut bytes).encode_frames(frames).unwrap();
    bytes
}
