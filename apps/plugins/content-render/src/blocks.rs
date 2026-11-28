// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Converts embedded HTML conventions into renderable Markdown blocks.

use html5ever::local_name;
use markdown::mdast::{Node, Paragraph};
use markup5ever_rcdom::{Handle, NodeData};

use super::html::{fragment, inner_html};

/// A block with presentation semantics that require custom Markdown rendering.
#[derive(Debug, PartialEq, Eq)]
pub enum Block {
    CenteredHtml(String),
    Image(ImageBlock),
    Superscript(Vec<Superscript>),
}

/// Standalone image metadata, including optional HTML dimensions in CSS pixels.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ImageBlock {
    src: String,
    alt: String,
    title: Option<String>,
    link: Option<String>,
    width: Option<u16>,
    height: Option<u16>,
    centered: bool,
}

impl ImageBlock {
    pub fn src(&self) -> &str {
        &self.src
    }

    pub fn alt(&self) -> &str {
        &self.alt
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn link(&self) -> Option<&str> {
        self.link.as_deref()
    }

    pub fn width(&self) -> Option<u16> {
        self.width
    }

    pub fn height(&self) -> Option<u16> {
        self.height
    }

    pub fn is_centered(&self) -> bool {
        self.centered
    }
}

/// A standalone superscript note with its original inline Markdown and plain text.
#[derive(Debug, PartialEq, Eq)]
pub struct Superscript {
    markdown: String,
    text: String,
}

impl Superscript {
    pub fn markdown(&self) -> &str {
        &self.markdown
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Converts an extension block from a Markdown AST built from `source`.
pub fn parse_block(node: &Node, source: &str) -> Option<Block> {
    match node {
        Node::Html(html) => html_block(&html.value),
        Node::Paragraph(paragraph) => paragraph_image(paragraph)
            .map(Block::Image)
            .or_else(|| superscript_block(paragraph, source))
            .or_else(|| paragraph_html(paragraph, source)),
        _ => None,
    }
}

fn paragraph_image(paragraph: &Paragraph) -> Option<ImageBlock> {
    let (image, link) = match paragraph.children.as_slice() {
        [Node::Image(image)] => (image, None),
        [Node::Link(link)] => match link.children.as_slice() {
            [Node::Image(image)] => (image, Some(link.url.clone())),
            _ => return None,
        },
        _ => return None,
    };
    Some(ImageBlock {
        src: image.url.clone(),
        alt: image.alt.clone(),
        title: image.title.clone(),
        link,
        width: None,
        height: None,
        centered: false,
    })
}

fn paragraph_html(paragraph: &Paragraph, source: &str) -> Option<Block> {
    let position = paragraph.position.as_ref()?;
    if !paragraph
        .children
        .iter()
        .all(|node| matches!(node, Node::Html(_)) || is_whitespace(node))
    {
        return None;
    }
    html_block(source.get(position.start.offset..position.end.offset)?)
}

fn html_block(html: &str) -> Option<Block> {
    let html = html.trim_start();
    let tag_start = html.strip_prefix('<')?;
    let tag_end = tag_start.find(|character: char| !character.is_ascii_alphanumeric())?;
    let tag = &tag_start[..tag_end];
    if !tag.eq_ignore_ascii_case("img") && !tag.eq_ignore_ascii_case("center") {
        return None;
    }

    let dom = fragment(html);
    let root = only_content(&dom.document, false)?;
    let element = only_content(&root, false)?;
    if tag.eq_ignore_ascii_case("img") {
        return image(&element, false).map(Block::Image);
    }
    if let Some(image) = only_content(&element, true).and_then(|child| image(&child, true)) {
        return Some(Block::Image(image));
    }
    inner_html(element).map(Block::CenteredHtml)
}

fn only_content(parent: &Handle, skip_empty_paragraph: bool) -> Option<Handle> {
    let children = parent.children.borrow();
    let mut content = children
        .iter()
        .filter(|node| !is_empty(node, skip_empty_paragraph));
    let first = content.next()?;
    content.next().is_none().then(|| first.clone())
}

fn is_empty(node: &Handle, skip_empty_paragraph: bool) -> bool {
    match &node.data {
        NodeData::Text { contents } => contents.borrow().trim().is_empty(),
        NodeData::Comment { .. } => true,
        NodeData::Element { name, .. }
            if skip_empty_paragraph && name.local == local_name!("p") =>
        {
            node.children
                .borrow()
                .iter()
                .all(|node| is_empty(node, false))
        }
        _ => false,
    }
}

fn image(node: &Handle, centered: bool) -> Option<ImageBlock> {
    let NodeData::Element { name, attrs, .. } = &node.data else {
        return None;
    };
    if name.local != local_name!("img") {
        return None;
    }
    let attrs = attrs.borrow();
    let attr = |name| {
        attrs
            .iter()
            .find(|attr| attr.name.local == name)
            .map(|attr| &*attr.value)
    };
    let dimension = |name| attr(name).and_then(|value| value.trim().parse().ok());
    Some(ImageBlock {
        src: attr(local_name!("src"))?.to_owned(),
        alt: attr(local_name!("alt")).unwrap_or_default().to_owned(),
        title: attr(local_name!("title")).map(str::to_owned),
        link: None,
        width: dimension(local_name!("width")),
        height: dimension(local_name!("height")),
        centered,
    })
}

fn superscript_block(paragraph: &Paragraph, source: &str) -> Option<Block> {
    let mut children = paragraph.children.iter();
    let mut notes = Vec::new();
    while let Some(node) = children.next() {
        if is_whitespace(node) {
            continue;
        }
        if !is_tag(node, "<sup>") {
            return None;
        }
        notes.push(superscript(node, &mut children, source)?);
    }
    (!notes.is_empty()).then_some(Block::Superscript(notes))
}

fn superscript<'a>(
    opening: &Node,
    children: &mut impl Iterator<Item = &'a Node>,
    source: &str,
) -> Option<Superscript> {
    let start = opening.position()?.end.offset;
    let mut text = String::new();
    for child in children {
        if is_tag(child, "</sup>") {
            let end = child.position()?.start.offset;
            let markdown = source.get(start..end)?.to_owned();
            return Some(Superscript { markdown, text });
        }
        text.push_str(&child.to_string());
    }
    None
}

fn is_tag(node: &Node, tag: &str) -> bool {
    matches!(node, Node::Html(html) if html.value.eq_ignore_ascii_case(tag))
}

fn is_whitespace(node: &Node) -> bool {
    matches!(node, Node::Text(text) if text.value.trim().is_empty())
}

#[cfg(test)]
#[path = "../../../../tests/apps/plugins/content-render/src/blocks.rs"]
mod tests;
