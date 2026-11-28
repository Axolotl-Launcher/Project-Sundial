// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Adapts Markdown and HTML documents to GPUI's native text view.

use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    AnyElement, App, ClickEvent, Entity, IntoElement, MarkdownExtensions, MarkdownNode,
    MouseButton, SharedString, SharedUri, TextView, TextViewState, Window, div, v_flex,
};
use url::Url;

use super::blocks::{Block, ImageBlock, parse_block};
use super::html::ImageLinks;
use super::image_cache::{self, ImageCache, ImageFetcher};
use super::image_preview;
use super::images::ImageState;
use super::picture::picture;

const MARKDOWN_BLOCK: &str = "markdown-html-block";

/// The format of the provider's document source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentType {
    Markdown,
    Html,
}

/// Native document rendering with cached images loaded on demand.
pub struct ContentRenderer {
    fetch: ImageFetcher,
}

impl ContentRenderer {
    /// Supplies an asynchronous transport that returns image bytes or a diagnostic error.
    pub fn new<F, Request>(fetch: F) -> Self
    where
        F: Fn(String) -> Request + Send + Sync + 'static,
        Request: Future<Output = Result<Vec<u8>, String>> + Send + 'static,
    {
        Self {
            fetch: std::sync::Arc::new(move |url| Box::pin(fetch(url))),
        }
    }

    /// A complete source snapshot. The optional HTTP(S) base resolves relative links and images.
    pub fn document(
        &self,
        content_type: ContentType,
        source: &str,
        base_url: Option<Url>,
        cx: &mut App,
    ) -> ContentDocument {
        let resources = DocumentResources {
            base_url: base_url.filter(is_web_url).map(std::sync::Arc::new),
            images: ImageCache::new(self.fetch.clone(), cx),
        };
        // TextViewState::set_text may reparse only the tail and miss references
        // whose definitions were appended after earlier paragraphs.
        let html = (content_type == ContentType::Html)
            .then(|| ImageLinks::prepare(source))
            .flatten();
        let state = cx.new(|cx| match content_type {
            ContentType::Markdown => TextViewState::markdown(source, cx),
            ContentType::Html => {
                TextViewState::html(html.as_ref().map_or(source, |(html, _)| html), cx)
            }
        });
        ContentDocument {
            state,
            extensions: (content_type == ContentType::Markdown)
                .then(|| extensions(resources.clone())),
            resources,
            image_links: html.map(|(_, links)| links),
        }
    }
}

/// A retained document that can be mounted in a GPUI view.
pub struct ContentDocument {
    state: Entity<TextViewState>,
    resources: DocumentResources,
    extensions: Option<MarkdownExtensions>,
    image_links: Option<ImageLinks>,
}

impl ContentDocument {
    /// A native text view with this document's image and link handling.
    pub fn view(&self) -> TextView {
        let view = text_view(TextView::new(&self.state), &self.resources)
            .when_some(self.extensions.clone(), TextView::markdown_extensions);
        view.when_some(self.image_links.clone(), |view, links| {
            bind_image_links(view, links, &self.resources)
        })
    }

    /// Retries only failed image requests in this document.
    pub fn retry_images(&self, cx: &mut App) {
        self.resources
            .images
            .update(cx, |cache, cx| cache.retry(cx));
        self.state.update(cx, |_, cx| cx.notify());
    }
}

#[derive(Clone)]
struct DocumentResources {
    base_url: Option<std::sync::Arc<Url>>,
    images: Entity<ImageCache>,
}

impl DocumentResources {
    fn url(&self, raw: &str) -> Option<Url> {
        web_url(raw, self.base_url.as_deref())
    }

    fn image_uri(&self, raw: &str) -> Option<SharedUri> {
        self.url(raw).map(|mut url| {
            url.set_fragment(None);
            SharedUri::from(String::from(url))
        })
    }
}

fn extensions(resources: DocumentResources) -> MarkdownExtensions {
    MarkdownExtensions::default()
        .block_parser(|node, context| {
            let block = parse_block(node, context.source())?;
            let text = block_text(&block);
            Some(
                MarkdownNode::new(MARKDOWN_BLOCK, block)
                    .text(text)
                    .markdown(context.node_source(node)?),
            )
        })
        .block_renderer(MARKDOWN_BLOCK, move |node, window, cx| {
            render_block(node, &resources, window, cx)
        })
}

fn block_text(block: &Block) -> String {
    match block {
        Block::CenteredHtml(_) => String::new(),
        Block::Image(image) => image.alt().to_owned(),
        Block::Superscript(notes) => notes
            .iter()
            .flat_map(|note| note.text().chars().chain(std::iter::once('\n')))
            .collect(),
    }
}

fn text_view(view: TextView, resources: &DocumentResources) -> TextView {
    let images = resources.clone();
    let base = resources.base_url.clone();
    view.image_source(move |uri| {
        image_cache::source(images.image_uri(uri.as_ref()), images.images.clone())
    })
    .link(move |link, _| {
        Some(
            link.clone()
                .url(web_url(&link.url, base.as_deref())?.as_str()),
        )
    })
}

fn web_url(raw: &str, base: Option<&Url>) -> Option<Url> {
    Url::options()
        .base_url(base)
        .parse(raw)
        .ok()
        .filter(is_web_url)
}

fn is_web_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
}

fn bind_image_links(view: TextView, images: ImageLinks, resources: &DocumentResources) -> TextView {
    let links = images.clone();
    let base = resources.base_url.clone();
    let resources = resources.clone();
    view.link(move |link, _| {
        if links.image(&link.url).is_some() {
            return Some(link.clone());
        }
        Some(
            link.clone()
                .url(web_url(&link.url, base.as_deref())?.as_str()),
        )
    })
    .on_link_click(move |url, event, window, cx| {
        activate_link(&images, url, event, &resources, window, cx);
    })
}

fn activate_link(
    images: &ImageLinks,
    url: &str,
    event: &ClickEvent,
    resources: &DocumentResources,
    window: &mut Window,
    cx: &mut App,
) {
    let primary = match event {
        ClickEvent::Mouse(click) => click.up.button == MouseButton::Left,
        ClickEvent::Keyboard(_) => true,
        ClickEvent::Touch(click) => !click.long_press,
    };
    if let Some((uri, name)) = images.image(url) {
        if primary
            && let Some(uri) = resources.image_uri(uri.as_ref())
            && let Some(image) = ImageCache::loaded(&resources.images, uri, cx)
        {
            image_preview::open(image, name.clone(), window, cx);
        }
        return;
    }
    let middle =
        matches!(event, ClickEvent::Mouse(click) if click.up.button == MouseButton::Middle);
    if (primary || middle)
        && let Some(url) = resources.url(url)
    {
        cx.open_url(url.as_str());
    }
}

fn render_block(
    node: &MarkdownNode,
    resources: &DocumentResources,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Some(block) = node.data::<Block>() else {
        return div().into_any_element();
    };
    let start = node.source_range().map_or(0, |range| range.start);
    match block {
        Block::CenteredHtml(html) => v_flex()
            .w_full()
            .items_center()
            .child(
                text_view(
                    TextView::html(("centered-html", start), html.clone()),
                    resources,
                )
                .max_w_full(),
            )
            .into_any_element(),
        Block::Image(image) => render_image(image, start, resources, window, cx).into_any_element(),
        Block::Superscript(notes) => v_flex()
            .id(("superscript-notes", start))
            .children(notes.iter().enumerate().map(|(line, note)| {
                text_view(
                    TextView::markdown(("note", line), note.markdown().to_owned()),
                    resources,
                )
                .text_xs()
            }))
            .into_any_element(),
    }
}

fn render_image(
    image: &ImageBlock,
    start: usize,
    resources: &DocumentResources,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let name = if image.alt().is_empty() {
        image.title().unwrap_or_default()
    } else {
        image.alt()
    };
    let link = image
        .link()
        .and_then(|link| resources.url(link))
        .map(|url| SharedString::from(url.as_str()));
    let state = resources
        .image_uri(image.src())
        .map_or(ImageState::Unavailable, |uri| {
            ImageCache::state(&resources.images, uri, window, cx)
        });
    v_flex()
        .id(("image-block", start))
        .w_full()
        .min_w_0()
        .items_start()
        .when(image.is_centered(), |row| row.items_center())
        .child(picture(
            state,
            image.width(),
            image.height(),
            name.to_owned().into(),
            link,
            cx,
        ))
}
