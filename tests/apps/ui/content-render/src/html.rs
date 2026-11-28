// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::ImageLinks;

#[test]
fn makes_standalone_and_inline_html_images_previewable() {
    let source = r#"<p><img src="https://media.forgecdn.net/attachments/546/809/6.gif" width="420" alt="Jade &amp; HUD"></p>
<p>See <strong><img src="https://example.com/preview.png" title="Details"></strong> here.</p>"#;
    let (html, links) = ImageLinks::prepare(source).expect("preview links");
    let (url, name) = links.image("axolotl-image-preview:0").expect("first image");
    assert_eq!(
        url.as_ref(),
        "https://media.forgecdn.net/attachments/546/809/6.gif"
    );
    assert_eq!(name.as_ref(), "Jade & HUD");
    let (_, name) = links
        .image("axolotl-image-preview:1")
        .expect("second image");
    assert_eq!(name.as_ref(), "Details");
    assert!(html.contains("<p><a href=\"axolotl-image-preview:0\"><img"));
    assert!(html.contains("width=\"420\""));
    assert!(html.contains("See <strong><a href=\"axolotl-image-preview:1\"><img"));
    assert!(html.contains("</a></strong> here."));
}

#[test]
fn preserves_image_links_and_ignores_author_supplied_preview_actions() {
    let source = r#"<a href="https://example.com/details"><img src="badge.png"></a>
<a href="axolotl-image-preview:&#48;">external text</a><img src="screenshot.png">
<img alt="no source"><img src=" ">"#;
    let (html, links) = ImageLinks::prepare(source).expect("preview links");
    assert!(html.contains(r#"<a href="https://example.com/details"><img src="badge.png"></a>"#));
    assert!(html.contains("<a>external text</a>"));
    assert!(links.image("axolotl-image-preview:1").is_none());
    let (url, _) = links.image("axolotl-image-preview:0").expect("screenshot");
    assert_eq!(url.as_ref(), "screenshot.png");
}

#[test]
fn keeps_documents_without_preview_actions_on_the_native_html_path() {
    assert!(ImageLinks::prepare(r#"<p>&lt;img src="example.png"&gt;</p>"#).is_none());
    assert!(
        ImageLinks::prepare(r#"<a href="https://example.com"><img src="badge.png"></a>"#).is_none()
    );
}
