// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Rasterizes SVG image responses in the document's decode budget.

use std::sync::LazyLock;

use axlauncher_ui::swap_rgba_pa_to_bgra;
use image::error::{DecodingError, ImageFormatHint};
use image::{Frame, ImageError, ImageResult, Limits, RgbaImage};
use resvg::{tiny_skia, usvg};
use roxmltree::{Document, ParsingOptions};

use super::{decoded_limit, reserve_bytes};

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
const MAX_XML_NODES: u32 = 16_384;

static OPTIONS: LazyLock<usvg::Options<'static>> = LazyLock::new(|| {
    let mut options = usvg::Options {
        // SVG image references would escape the document's transport and pixel budget.
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    options.fontdb_mut().load_system_fonts();
    let select_font = usvg::FontResolver::default_font_selector();
    options.font_resolver.select_font = Box::new(move |font, db| {
        select_font(font, db).or_else(|| db.faces().next().map(|face| face.id))
    });
    options
});

pub(super) fn xml_source(bytes: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text.trim_start_matches('\u{feff}').trim_start();
    text.starts_with('<').then_some(text)
}

pub(super) fn decode(source: &str, limits: &Limits, remaining: &mut usize) -> ImageResult<Frame> {
    let xml = Document::parse_with_options(
        source,
        ParsingOptions {
            nodes_limit: MAX_XML_NODES,
            ..Default::default()
        },
    )
    .map_err(svg_error)?;
    if !xml.root_element().has_tag_name((SVG_NAMESPACE, "svg")) {
        return Err(svg_error("Expected an SVG document"));
    }
    let tree = usvg::Tree::from_xmltree(&xml, &OPTIONS).map_err(svg_error)?;
    let size = tree.size().to_int_size();
    let (width, height) = (size.width(), size.height());
    limits.check_dimensions(width, height)?;
    reserve_bytes(remaining, width as usize * height as usize * 4)?;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or_else(decoded_limit)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    let mut pixels = pixmap.take();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        swap_rgba_pa_to_bgra(pixel);
    }
    let pixels = RgbaImage::from_raw(width, height, pixels).expect("pixmap contains RGBA pixels");
    Ok(Frame::new(pixels))
}

fn svg_error(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> ImageError {
    ImageError::Decoding(DecodingError::new(
        ImageFormatHint::Name("SVG".into()),
        error,
    ))
}
