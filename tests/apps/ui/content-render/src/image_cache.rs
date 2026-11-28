// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::TestAppContext;
use image::codecs::gif::GifEncoder;
use image::{Frame, RgbaImage};

use super::{DocumentImage, ImageCache, MAX_DECODED_BYTES, reserve_bytes};

#[test]
fn decoded_images_cannot_exceed_the_document_budget() {
    let used = AtomicUsize::new(0);
    assert!(reserve_bytes(&used, MAX_DECODED_BYTES - 1));
    assert!(!reserve_bytes(&used, 2));
    assert!(reserve_bytes(&used, 1));
    assert!(!reserve_bytes(&used, usize::MAX));
    assert_eq!(used.load(Ordering::Relaxed), MAX_DECODED_BYTES);
}

#[test]
fn releasing_one_document_evicts_only_its_image_assets() {
    let cx = TestAppContext::single();
    let fetch = |_: &str| {
        let mut bytes = Vec::new();
        let frames = [
            Frame::new(RgbaImage::new(1, 1)),
            Frame::new(RgbaImage::new(1, 1)),
        ];
        GifEncoder::new(&mut bytes).encode_frames(frames).unwrap();
        Ok(bytes)
    };
    let (document, other, request, other_request) = cx.update(|cx| {
        let document = ImageCache::new(fetch, cx);
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
        assert_eq!(document.read(cx).decoded_bytes.load(Ordering::Relaxed), 8);
        assert_eq!(other.read(cx).decoded_bytes.load(Ordering::Relaxed), 8);
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
