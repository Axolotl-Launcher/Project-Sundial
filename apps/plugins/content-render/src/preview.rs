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
use super::image_preview;
use super::images::{self, ImageFetcher, ImageRequest};
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
    extensions: MarkdownExtensions,
}

impl ContentRenderer {
    /// The transport runs on a background thread and returns bytes or an unavailable image.
    pub fn new(fetch: fn(&str) -> Option<Vec<u8>>) -> Self {
        let extensions = MarkdownExtensions::default()
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
                render_block(node, fetch, window, cx)
            });
        Self { fetch, extensions }
    }

    /// A complete source snapshot with its text state and rendering configuration.
    pub fn document(
        &self,
        content_type: ContentType,
        source: &str,
        cx: &mut App,
    ) -> ContentDocument {
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
            fetch: self.fetch,
            extensions: (content_type == ContentType::Markdown).then(|| self.extensions.clone()),
            image_links: html.map(|(_, links)| links),
        }
    }
}

/// A retained document that can be mounted in a GPUI view.
pub struct ContentDocument {
    state: Entity<TextViewState>,
    fetch: ImageFetcher,
    extensions: Option<MarkdownExtensions>,
    image_links: Option<ImageLinks>,
}

impl ContentDocument {
    /// A native text view with this document's image and link handling.
    pub fn view(&self) -> TextView {
        let view = text_view(TextView::new(&self.state), self.fetch)
            .when_some(self.extensions.clone(), TextView::markdown_extensions);
        view.when_some(self.image_links.clone(), |view, links| {
            bind_image_links(view, links, self.fetch)
        })
    }
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

fn text_view(view: TextView, fetch: ImageFetcher) -> TextView {
    view.image_source(move |uri| images::source(uri, fetch))
        .link(|link, _| Some(link.clone().url(web_url(&link.url)?.as_str())))
}

fn web_url(raw: &str) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

fn bind_image_links(view: TextView, images: ImageLinks, fetch: ImageFetcher) -> TextView {
    let links = images.clone();
    view.link(move |link, _| {
        if links.image(&link.url).is_some() {
            return Some(link.clone());
        }
        Some(link.clone().url(web_url(&link.url)?.as_str()))
    })
    .on_link_click(move |url, event, window, cx| {
        activate_link(&images, url, event, fetch, window, cx);
    })
}

fn activate_link(
    images: &ImageLinks,
    url: &str,
    event: &ClickEvent,
    fetch: ImageFetcher,
    window: &mut Window,
    cx: &mut App,
) {
    let primary = match event {
        ClickEvent::Mouse(click) => click.up.button == MouseButton::Left,
        ClickEvent::Keyboard(_) => true,
        ClickEvent::Touch(click) => !click.long_press,
    };
    if let Some((uri, name)) = images.image(url) {
        if primary && let Some(image) = ImageRequest::new(uri.clone(), fetch).loaded(cx) {
            image_preview::open(image, name.clone(), window, cx);
        }
        return;
    }
    let middle =
        matches!(event, ClickEvent::Mouse(click) if click.up.button == MouseButton::Middle);
    if (primary || middle)
        && let Some(url) = web_url(url)
    {
        cx.open_url(url.as_str());
    }
}

fn render_block(
    node: &MarkdownNode,
    fetch: ImageFetcher,
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
                    fetch,
                )
                .max_w_full(),
            )
            .into_any_element(),
        Block::Image(image) => render_image(image, start, fetch, window, cx).into_any_element(),
        Block::Superscript(notes) => v_flex()
            .id(("superscript-notes", start))
            .children(notes.iter().enumerate().map(|(line, note)| {
                text_view(
                    TextView::markdown(("note", line), note.markdown().to_owned()),
                    fetch,
                )
                .text_xs()
            }))
            .into_any_element(),
    }
}

fn render_image(
    image: &ImageBlock,
    start: usize,
    fetch: ImageFetcher,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let request = ImageRequest::new(SharedUri::from(image.src().to_owned()), fetch);
    let name = if image.alt().is_empty() {
        image.title().unwrap_or_default()
    } else {
        image.alt()
    };
    let link = image
        .link()
        .and_then(web_url)
        .map(|url| SharedString::from(url.as_str()));
    let state = request.state(window, cx);
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
            window,
            cx,
        ))
}
