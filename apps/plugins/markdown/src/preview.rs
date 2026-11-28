// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Adapts Markdown extension blocks to GPUI's native text view.

use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    AnyElement, App, Entity, IntoElement, MarkdownExtensions, MarkdownNode, SharedString,
    SharedUri, TextView, TextViewState, Window, div, v_flex,
};
use url::Url;

use super::blocks::{Block, ImageBlock, parse_block};
use super::images::{self, ImageFetcher, ImageRequest};
use super::picture::picture;

const MARKDOWN_BLOCK: &str = "markdown-html-block";

/// Native Markdown rendering with cached images loaded on demand.
pub struct Markdown {
    fetch: ImageFetcher,
    extensions: MarkdownExtensions,
}

impl Markdown {
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

    /// Parses a complete source document into retained GPUI text state.
    pub fn parse(&self, source: &str, cx: &mut App) -> Entity<TextViewState> {
        // TextViewState::set_text may reparse only the tail and miss references
        // whose definitions were appended after earlier paragraphs.
        cx.new(|cx| TextViewState::markdown(source, cx))
    }

    /// Builds a GPUI text view for the caller's Markdown document state.
    pub fn text_view(&self, state: &Entity<TextViewState>) -> TextView {
        document(TextView::new(state), self.fetch).markdown_extensions(self.extensions.clone())
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

fn document(view: TextView, fetch: ImageFetcher) -> TextView {
    view.image_source(move |uri| images::source(uri, fetch))
        .link(|link, _| Some(link.clone().url(web_url(&link.url)?.as_str())))
}

fn web_url(raw: &str) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
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
                document(
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
                document(
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
    let loaded = request.loaded(window, cx);
    let placeholder = loaded
        .is_none()
        .then(|| images::placeholder(window, cx))
        .flatten();
    v_flex()
        .id(("image-block", start))
        .w_full()
        .min_w_0()
        .items_start()
        .when(image.is_centered(), |row| row.items_center())
        .child(picture(
            loaded,
            placeholder,
            image.width(),
            image.height(),
            name.to_owned().into(),
            link,
            cx,
        ))
}
