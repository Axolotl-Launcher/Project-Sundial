// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};

use axlauncher_content_render::{ContentDocument, ContentRenderer, ContentType};
use axlauncher_ui::{
    AppContext as _, Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowExt as _,
    WindowOptions, kit, point, px, size,
};
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use url::Url;

static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn retries_a_failed_relative_image_then_opens_the_preview() {
    let cx = TestAppContext::single();
    let (handle, view) = mount(
        &cx,
        retryable_image,
        r#"<p><img src="images/badge.png#preview" width="64" height="48" alt="Badge"></p>"#,
    );
    let mut window = VisualTestContext::from_window(handle, &cx);
    window.run_until_parked();
    assert_eq!(ATTEMPTS.load(Ordering::SeqCst), 1);
    window.simulate_click(point(px(20.), px(20.)), Modifiers::default());
    window.update(|window, cx| assert!(!window.has_active_dialog(cx)));

    cx.update(|cx| view.update(cx, |view, cx| view.0.retry_images(cx)));
    window.run_until_parked();
    assert_eq!(ATTEMPTS.load(Ordering::SeqCst), 2);
    window.simulate_click(point(px(20.), px(20.)), Modifiers::default());
    window.update(|window, cx| assert!(window.has_active_dialog(cx)));
    cx.update(|cx| view.update(cx, |view, cx| view.0.retry_images(cx)));
    window.run_until_parked();
    assert_eq!(ATTEMPTS.load(Ordering::SeqCst), 2);
    cx.quit();
}

#[test]
fn linked_html_image_opens_the_resolved_destination() {
    let cx = TestAppContext::single();
    let (handle, _) = mount(
        &cx,
        |url| async move {
            assert_eq!(url, "https://example.com/projects/images/badge.png");
            png()
        },
        r#"<p><a href="/linkout?target=forge"><img src="images/badge.png" width="64" height="48" alt="Forge"></a></p>"#,
    );
    let mut window = VisualTestContext::from_window(handle, &cx);
    window.run_until_parked();
    window.simulate_click(point(px(20.), px(20.)), Modifiers::default());
    assert_eq!(
        cx.opened_url().as_deref(),
        Some("https://example.com/linkout?target=forge")
    );
    cx.quit();
}

struct Preview(ContentDocument);

impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        self.0.view().scrollable(true)
    }
}

fn mount<Request: Future<Output = Result<Vec<u8>, String>> + Send + 'static>(
    cx: &TestAppContext,
    fetch: fn(String) -> Request,
    html: &str,
) -> (
    axlauncher_ui::AnyWindowHandle,
    axlauncher_ui::Entity<Preview>,
) {
    cx.update(|cx| {
        kit::init(cx);
        kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(600.), px(400.)),
                ))),
                ..Default::default()
            },
            cx,
            |_, cx| {
                let base = Url::parse("https://example.com/projects/create").unwrap();
                let document =
                    ContentRenderer::new(fetch).document(ContentType::Html, html, Some(base), cx);
                cx.new(|_| Preview(document))
            },
        )
        .unwrap()
    })
}

async fn retryable_image(url: String) -> Result<Vec<u8>, String> {
    assert_eq!(url, "https://example.com/projects/images/badge.png");
    if ATTEMPTS.fetch_add(1, Ordering::SeqCst) == 0 {
        return Err("offline".into());
    }
    png()
}

fn png() -> Result<Vec<u8>, String> {
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(64, 48)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(bytes.into_inner())
}
