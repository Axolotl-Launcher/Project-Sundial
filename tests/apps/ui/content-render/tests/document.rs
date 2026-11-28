// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axlauncher_content_render::{ContentDocument, ContentRenderer, ContentType, ImageFetchError};
use axlauncher_ui::{
    AppContext as _, Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowExt as _,
    WindowOptions, kit, point, px, size,
};
use gpui::{KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, TestAppContext, VisualTestContext};
use url::Url;

static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
const SVG_BADGE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48"><rect width="64" height="48" fill="#4c1"/></svg>"##;

#[test]
fn retries_a_failed_relative_image_then_opens_the_preview() {
    let cx = TestAppContext::single();
    let (handle, view) = mount(
        &cx,
        retryable_image,
        ContentType::Html,
        r#"<p><img src="images/badge#preview" width="64" height="48" alt="Badge"></p>"#,
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
            Ok(png())
        },
        ContentType::Html,
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

#[test]
fn opens_a_downloaded_svg_in_the_markdown_image_preview() {
    let cx = TestAppContext::single();
    let (handle, _) = mount(
        &cx,
        |_| async { Ok(SVG_BADGE.to_vec()) },
        ContentType::Markdown,
        "![Badge](images/badge.svg)",
    );
    let mut window = VisualTestContext::from_window(handle, &cx);
    window.run_until_parked();
    window.simulate_click(point(px(20.), px(20.)), Modifiers::default());
    window.update(|window, cx| assert!(window.has_active_dialog(cx)));
    cx.quit();
}

#[test]
fn enter_opens_the_destination_of_a_focused_markdown_image_link() {
    for available in [true, false] {
        let cx = TestAppContext::single();
        let (handle, _) = mount(
            &cx,
            move |url| markdown_badge(url, available),
            ContentType::Markdown,
            "[![Badge](images/badge.png)](/target)",
        );
        let mut window = VisualTestContext::from_window(handle, &cx);
        window.run_until_parked();
        window.update(|window, cx| {
            window.focus_next(cx);
            assert!(
                window.focused(cx).is_some(),
                "the link should be a tab stop"
            );
            window.draw(cx).clear(cx);
        });
        assert_eq!(cx.opened_url(), None);
        press_key(&mut window, "enter");
        assert_eq!(
            cx.opened_url().as_deref(),
            Some("https://example.com/target")
        );
        cx.quit();
    }
}

async fn markdown_badge(url: String, available: bool) -> Result<Vec<u8>, ImageFetchError> {
    assert_eq!(url, "https://example.com/projects/images/badge.png");
    if available {
        Ok(png())
    } else {
        Err(ImageFetchError::permanent("offline"))
    }
}

fn press_key(window: &mut VisualTestContext, key: &str) {
    let keystroke =
        window.update(|_, cx| Keystroke::parse_with_os(key, cx.platform_info().os).unwrap());
    window.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    window.simulate_event(KeyUpEvent { keystroke });
}

struct Preview(ContentDocument);

impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        self.0.view().scrollable(true)
    }
}

fn mount<Request: Future<Output = Result<Vec<u8>, ImageFetchError>> + Send + 'static>(
    cx: &TestAppContext,
    fetch: impl Fn(String) -> Request + Send + Sync + 'static,
    content_type: ContentType,
    source: &str,
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
                    ContentRenderer::new(fetch).document(content_type, source, Some(base), cx);
                cx.new(|_| Preview(document))
            },
        )
        .unwrap()
    })
}

async fn retryable_image(url: String) -> Result<Vec<u8>, ImageFetchError> {
    assert_eq!(url, "https://example.com/projects/images/badge");
    if ATTEMPTS.fetch_add(1, Ordering::SeqCst) == 0 {
        return Err(ImageFetchError::retryable("offline", Duration::ZERO));
    }
    Ok(SVG_BADGE.to_vec())
}

fn png() -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(64, 48)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}
