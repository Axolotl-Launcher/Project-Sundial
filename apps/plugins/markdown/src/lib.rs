// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Renders Markdown in GPUI and loads document images through a supplied transport.

#![forbid(unsafe_code)]

mod blocks;
mod image_preview;
mod images;
mod picture;
mod preview;

pub use preview::Markdown;
