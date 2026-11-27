// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Axolotl Launcher UI layer: design tokens and the component vocabulary the
//! frontend is allowed to use.
//!
//! Presentation comes from [GPUI Kit](https://gpui-kit.com) — its `gpui-base`
//! behaviour layer, its `gpui-component` styled components, and the Lucide
//! assets bundle. This crate draws nothing itself: it re-exports the kit pieces
//! under one import path so the frontend never reaches into kit internals, and
//! it owns the app-specific palette.
//!
//! Swapping the component system later means rewriting this crate alone.

pub use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, IndexPath, Sizable, Size, StyledExt, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::{Input, InputEvent, InputState, NumberInput, Textarea, TextareaState},
    label::Label,
    link::Link,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    pagination::Pagination,
    radio::RadioGroup,
    scroll::ScrollableElement,
    select::{SearchableVec, Select, SelectDelegate, SelectEvent, SelectItem, SelectState},
    separator::Separator,
    sidebar::{Sidebar, SidebarMenu},
    slider::{Slider, SliderEvent, SliderState},
    switch::Switch,
    tooltip::Tooltip,
    v_flex,
};
pub use gpui_kit::{self as kit, *};

/// Decodes the embedded application icon for the X11 window backend.
#[cfg(target_os = "linux")]
pub fn application_icon() -> std::sync::Arc<image::RgbaImage> {
    let png = include_bytes!("../../../packaging/icons/axolotl-launcher.png");
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .expect("decode the embedded Axolotl Launcher icon");
    std::sync::Arc::new(image.into_rgba8())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Palette {
    Dark,
    Light,
}

impl Palette {
    pub fn mode(self) -> kit::component::ThemeMode {
        match self {
            Palette::Dark => kit::component::ThemeMode::Dark,
            Palette::Light => kit::component::ThemeMode::Light,
        }
    }

    /// Initialises every kit layer and installs this palette. Must run once
    /// before any component renders.
    pub fn install(self, cx: &mut kit::App) {
        kit::init(cx);
        self.apply(cx);
    }

    /// Applies this palette to the running application without re-initialising
    /// the kit.
    pub fn apply(self, cx: &mut kit::App) {
        kit::component::Theme::change(self.mode(), None, cx);
    }
}
