// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn catalogs_have_the_same_nonempty_keys_and_named_arguments() {
    assert!(EN.keys().eq(ZH.keys()));
    for (key, source) in EN.iter() {
        let translation = &ZH[key];
        assert!(
            !source.trim().is_empty() && !translation.trim().is_empty(),
            "{key}"
        );
        assert_eq!(arguments(source), arguments(translation), "{key}");
    }
}

fn arguments(message: &str) -> Vec<&str> {
    let mut names = message
        .split('{')
        .skip(1)
        .map(|part| {
            let (name, _) = part.split_once('}').expect("named argument closes");
            assert!(!name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'));
            name
        })
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}

#[test]
fn setting_language_changes_lookup_without_changing_keys() {
    let key = "navigation.settings";
    assert_eq!(text(LauncherLanguage::English, key), "Settings");
    assert_eq!(text(LauncherLanguage::SimplifiedChinese, key), "设置");
}

#[test]
fn named_arguments_can_be_reordered_without_expanding_their_values() {
    assert_eq!(
        interpolate(
            "{right}：{left} / {right}",
            &[("left", "{right}"), ("right", "2")]
        ),
        "2：{right} / 2",
    );
}
