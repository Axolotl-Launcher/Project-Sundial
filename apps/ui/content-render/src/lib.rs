// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Renders Markdown and HTML in GPUI with a supplied image transport.

#![forbid(unsafe_code)]

mod blocks;
mod html;
mod image_preview;
mod images;
mod picture;
mod preview;

pub use preview::{ContentDocument, ContentRenderer, ContentType};
