// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Superlogical UI layer: design tokens and the component vocabulary the
//! frontend is allowed to use.
//!
//! Presentation comes from [GPUI Kit](https://gpui-kit.com) — its `gpui-base`
//! behaviour layer, its `gpui-component` styled components, and the Lucide
//! assets bundle. This crate draws nothing itself: it re-exports the kit pieces
//! under one import path so the frontend never reaches into kit internals, and
//! it owns the app-specific palette.
//!
//! Swapping the component system later means rewriting this crate alone.

/// Components and the layout/text helpers they need, in kit's own module
/// layout.
pub use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable, Size, StyledExt,
    button::{Button, ButtonVariants},
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    label::Label,
    separator::Separator,
    v_flex,
};
/// GPUI itself, as the kit re-exports it.
pub use gpui_kit::{self as kit, *};

/// The application palette.
///
/// GPUI Kit owns the underlying token tree (`gpui_component::Theme`); this type
/// maps Superlogical's choice of appearance onto it. A repaint of the whole app
/// starts here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Palette {
    /// Dark chrome, the default.
    Dark,
    /// Light chrome.
    Light,
}

impl Palette {
    /// The theme mode to install on the kit.
    pub fn mode(self) -> kit::component::ThemeMode {
        match self {
            Palette::Dark => kit::component::ThemeMode::Dark,
            Palette::Light => kit::component::ThemeMode::Light,
        }
    }

    /// Initialises every kit layer and installs this palette.
    ///
    /// Must run once before any component renders; opens no window.
    pub fn install(self, cx: &mut kit::App) {
        kit::init(cx);
        kit::component::Theme::change(self.mode(), None, cx);
    }
}
