// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Adds image preview actions to GPUI's native HTML image nodes.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use axlauncher_ui::{SharedString, SharedUri};
use html5ever::serialize::SerializeOpts;
use html5ever::tendril::TendrilSink as _;
use html5ever::{Attribute, LocalName, ParseOpts, QualName};
use html5ever::{local_name, namespace_url, ns, parse_fragment, serialize};
use markup5ever_rcdom::{Handle, Node, NodeData, RcDom, SerializableHandle};

const PREVIEW_LINK: &str = "axolotl-image-preview:";

#[derive(Clone)]
pub(super) struct ImageLinks(Arc<[PreviewImage]>);

struct PreviewImage {
    uri: SharedUri,
    name: SharedString,
}

impl ImageLinks {
    pub(super) fn prepare(source: &str) -> Option<(String, Self)> {
        let dom = fragment(source);
        let root = dom.document.children.borrow().first()?.clone();
        let mut images = Vec::new();
        wrap_images(&root, false, &mut images);
        if images.is_empty() {
            return None;
        }
        let source = inner_html(root)?;
        Some((source, Self(images.into())))
    }

    pub(super) fn image(&self, link: &str) -> Option<(&SharedUri, &SharedString)> {
        let image = self.0.get(preview_index(link)?)?;
        Some((&image.uri, &image.name))
    }
}

fn preview_index(link: &str) -> Option<usize> {
    link.strip_prefix(PREVIEW_LINK)?.parse().ok()
}

fn wrap_images(parent: &Handle, inside_link: bool, images: &mut Vec<PreviewImage>) {
    for child in parent.children.borrow_mut().iter_mut() {
        wrap_image(child, inside_link, images);
    }
}

fn wrap_image(node: &mut Handle, inside_link: bool, images: &mut Vec<PreviewImage>) {
    let NodeData::Element { name, attrs, .. } = &node.data else {
        return;
    };
    let is_link = name.local == local_name!("a");
    if is_link {
        // Only links generated for this document may invoke preview actions.
        attrs.borrow_mut().retain(|attr| {
            attr.name.local != local_name!("href") || preview_index(attr.value.trim()).is_none()
        });
    }
    if inside_link || name.ns != ns!(html) || name.local != local_name!("img") {
        wrap_images(node, inside_link || is_link, images);
        return;
    }
    let Some(image) = preview_image(&attrs.borrow()) else {
        return;
    };
    *node = image_link(node.clone(), images.len());
    images.push(image);
}

fn preview_image(attrs: &[Attribute]) -> Option<PreviewImage> {
    let src = attribute(attrs, local_name!("src"))?;
    if src.trim().is_empty() {
        return None;
    }
    let name = attribute(attrs, local_name!("alt"))
        .filter(|alt| !alt.trim().is_empty())
        .or_else(|| attribute(attrs, local_name!("title")))
        .unwrap_or_default();
    Some(PreviewImage {
        uri: src.to_owned().into(),
        name: name.to_owned().into(),
    })
}

fn attribute(attrs: &[Attribute], name: LocalName) -> Option<&str> {
    attrs
        .iter()
        .find(|attr| attr.name.local == name)
        .map(|attr| &*attr.value)
}

fn image_link(image: Handle, index: usize) -> Handle {
    // TextView exposes activation for linked images, including images inside text and tables.
    let wrapper = Node::new(NodeData::Element {
        name: QualName::new(None, ns!(html), local_name!("a")),
        attrs: RefCell::new(vec![Attribute {
            name: QualName::new(None, ns!(), local_name!("href")),
            value: format!("{PREVIEW_LINK}{index}").into(),
        }]),
        template_contents: RefCell::new(None),
        mathml_annotation_xml_integration_point: false,
    });
    wrapper
        .parent
        .set(image.parent.replace(Some(Rc::downgrade(&wrapper))));
    wrapper.children.borrow_mut().push(image);
    wrapper
}

pub(super) fn fragment(source: &str) -> RcDom {
    parse_fragment(
        RcDom::default(),
        ParseOpts::default(),
        QualName::new(None, ns!(html), local_name!("div")),
        vec![],
    )
    .one(source)
}

pub(super) fn inner_html(node: Handle) -> Option<String> {
    let mut contents = Vec::new();
    serialize(
        &mut contents,
        &SerializableHandle::from(node),
        SerializeOpts::default(),
    )
    .ok()?;
    String::from_utf8(contents).ok()
}

#[cfg(test)]
#[path = "../../../../tests/apps/ui/content-render/src/html.rs"]
mod tests;
