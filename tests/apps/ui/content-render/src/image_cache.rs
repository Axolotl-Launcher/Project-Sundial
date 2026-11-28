// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_lock::Semaphore;
use gpui::TestAppContext;
use image::codecs::gif::GifEncoder;
use image::{Frame, RgbaImage};

use super::{DocumentImage, ImageCache, ImageFetcher, MAX_DECODED_BYTES};

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
        let requests = ["https://example.com/one.gif", "https://example.com/two.gif"].map(|url| {
            let request = document.update(cx, |cache, _| cache.admit(url.into()).unwrap());
            assert!(cx.fetch_asset::<DocumentImage>(&request).is_none());
            request
        });
        (document, requests)
    });
    cx.run_until_parked();
    assert_eq!(started.load(Ordering::Relaxed), 2);
    finish.add_permits(2);
    cx.run_until_parked();
    cx.update(|cx| {
        for request in &requests {
            assert!(cx.fetch_asset::<DocumentImage>(request).flatten().is_some());
        }
        assert_eq!(
            *document.read(cx).remaining_bytes.try_lock().unwrap(),
            MAX_DECODED_BYTES - 16,
        );
    });
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
            Some(Some(_))
        ));
        assert!(matches!(
            cx.fetch_asset::<DocumentImage>(&other_request),
            Some(Some(_))
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

fn animated_image() -> Vec<u8> {
    let mut bytes = Vec::new();
    let frames = [
        Frame::new(RgbaImage::new(1, 1)),
        Frame::new(RgbaImage::new(1, 1)),
    ];
    GifEncoder::new(&mut bytes).encode_frames(frames).unwrap();
    bytes
}
