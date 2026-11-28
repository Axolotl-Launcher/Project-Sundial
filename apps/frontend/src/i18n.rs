// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Bundled desktop translations and the active language for each GPUI app.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use axlauncher_core::state::{LauncherLanguage, SettingsData};
use axlauncher_ui::{App, kit::Global};

const ENGLISH: &str = include_str!("../locales/en-US.json");
const CHINESE: &str = include_str!("../locales/zh-CN.json");

type Catalog = BTreeMap<String, String>;
static EN: LazyLock<Catalog> = LazyLock::new(|| catalog(ENGLISH));
static ZH: LazyLock<Catalog> = LazyLock::new(|| catalog(CHINESE));

fn catalog(json: &str) -> Catalog {
    serde_json::from_str(json).expect("bundled desktop translation catalog must be valid JSON")
}

struct ActiveLanguage(LauncherLanguage);

impl Global for ActiveLanguage {}

pub(crate) fn configured_language(settings: Option<&SettingsData>) -> LauncherLanguage {
    settings
        .map(|data| data.launcher_language)
        .unwrap_or_default()
}

pub(crate) fn language(cx: &App) -> LauncherLanguage {
    cx.try_global::<ActiveLanguage>()
        .map(|active| active.0)
        .unwrap_or_default()
}

pub(crate) fn set_language(language: LauncherLanguage, cx: &mut App) {
    if self::language(cx) != language {
        cx.set_global(ActiveLanguage(language));
        cx.refresh_windows();
    }
}

pub(crate) fn tr(cx: &App, key: &'static str) -> &'static str {
    text(language(cx), key)
}

pub(crate) fn text(language: LauncherLanguage, key: &'static str) -> &'static str {
    let messages = match language {
        LauncherLanguage::English => &*EN,
        LauncherLanguage::SimplifiedChinese => &*ZH,
    };
    messages
        .get(key)
        .filter(|message| !message.is_empty())
        .or_else(|| EN.get(key))
        .map_or(key, String::as_str)
}

pub(crate) fn interpolate(template: &str, arguments: &[(&str, &str)]) -> String {
    let mut result = String::with_capacity(template.len());
    let mut tail = template;
    while let Some((literal, rest)) = tail.split_once('{') {
        let Some((name, remainder)) = rest.split_once('}') else {
            break;
        };
        result.push_str(literal);
        if let Some((_, value)) = arguments.iter().find(|(argument, _)| *argument == name) {
            result.push_str(value);
        } else {
            result.push('{');
            result.push_str(name);
            result.push('}');
        }
        tail = remainder;
    }
    result.push_str(tail);
    result
}

#[cfg(test)]
#[path = "../../../tests/apps/frontend/src/i18n.rs"]
mod tests;
