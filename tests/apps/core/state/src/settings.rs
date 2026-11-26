// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::test_support::TempRoot;

#[test]
fn concurrent_updates_keep_both_language_and_theme() {
    let temp = TempRoot::new();
    let root = temp.path();
    load_settings(root).unwrap();
    let start = std::sync::Barrier::new(2);
    let language = |settings: &mut SettingsData| settings.default_game_language = "zh_cn".into();
    let theme = |settings: &mut SettingsData| settings.theme = ThemeChoice::Light;
    std::thread::scope(|scope| {
        scope.spawn(|| {
            start.wait();
            update_settings(root, theme).unwrap();
        });
        scope.spawn(|| {
            start.wait();
            update_settings(root, language).unwrap();
        });
    });
    let saved = load_settings(root).unwrap().data;
    assert_eq!(saved.theme, ThemeChoice::Light);
    assert_eq!(saved.default_game_language, "zh_cn");
}

fn seed(root: &Path, contents: &str) -> PathBuf {
    let path = root.join("settings.json");
    fs::write(&path, contents).expect("seed");
    path
}

#[test]
fn a_missing_file_is_created_with_defaults() {
    let temp = TempRoot::new();

    let loaded = load_settings(temp.path()).expect("load");

    assert_eq!(loaded.origin, SettingsOrigin::Created);
    assert_eq!(loaded.data, SettingsData::default());

    let written = fs::read_to_string(temp.path().join("settings.json")).expect("read back");
    assert!(written.contains(r#""schema_version": 1"#));
    assert!(written.contains(r#""theme": "dark""#));
}

#[test]
fn a_saved_theme_roundtrips() {
    let temp = TempRoot::new();
    let data = SettingsData {
        default_game_language: "zh_cn".into(),
        theme: ThemeChoice::Light,
        ..SettingsData::default()
    };

    save_settings(temp.path(), &data).expect("save");
    let loaded = load_settings(temp.path()).expect("load");

    assert_eq!(loaded.data, data);
    assert_eq!(loaded.origin, SettingsOrigin::Stored);
}

#[test]
fn fallen_proxy_defaults_to_off_and_can_be_enabled_in_existing_settings() {
    let temp = TempRoot::new();
    seed(
        temp.path(),
        r#"{"schema_version":1,"data":{"theme":"light"}}"#,
    );
    assert!(!load_settings(temp.path()).unwrap().data.use_fallen_proxy);
    update_settings(temp.path(), |settings| settings.use_fallen_proxy = true).unwrap();
    let loaded = load_settings(temp.path()).unwrap();
    assert!(loaded.data.use_fallen_proxy);
    assert_eq!(loaded.data.theme, ThemeChoice::Light);
}

#[test]
fn memory_defaults_to_automatic_and_manual_settings_roundtrip() {
    let temp = TempRoot::new();
    seed(temp.path(), r#"{"schema_version":1,"data":{}}"#);
    assert_eq!(
        load_settings(temp.path()).unwrap().data.memory(),
        MemoryAllocation::Automatic
    );
    let heap = axlauncher_domain::memory::MaximumHeap::new(6144).unwrap();
    update_settings(temp.path(), |settings| {
        settings.set_memory(MemoryAllocation::Manual(heap));
    })
    .unwrap();
    assert_eq!(
        load_settings(temp.path()).unwrap().data.memory(),
        MemoryAllocation::Manual(heap)
    );
    let written: serde_json::Value =
        serde_json::from_slice(&fs::read(settings_path(temp.path())).unwrap()).unwrap();
    assert_eq!(
        written["data"]["memory"],
        serde_json::json!({"mode": "manual", "maximum_mib": 6144})
    );
}

#[test]
fn an_unreadable_document_is_quarantined_and_defaults_apply() {
    let temp = TempRoot::new();
    seed(temp.path(), "{ not a document }");

    let loaded = load_settings(temp.path()).expect("load");

    assert_eq!(loaded.data, SettingsData::default());
    let SettingsOrigin::Quarantined { path } = &loaded.origin else {
        panic!("expected quarantine, got {:?}", loaded.origin);
    };
    assert_eq!(
        fs::read_to_string(path).expect("kept bytes"),
        "{ not a document }"
    );
    assert!(temp.path().join("settings.json").is_file());
}

#[test]
fn a_document_from_another_schema_is_read_only() {
    let temp = TempRoot::new();
    let contents = r#"{"schema_version":2,"data":{"theme":"light"}}"#;
    let path = seed(temp.path(), contents);
    let before = fs::read(&path).expect("before");

    let loaded = load_settings(temp.path()).expect("load");

    assert_eq!(
        loaded.origin,
        SettingsOrigin::ReadOnlyOtherSchema { found: 2 }
    );
    assert_eq!(loaded.data, SettingsData::default());
    assert_eq!(fs::read(&path).expect("after"), before);
}

#[test]
fn saving_over_another_schema_is_refused() {
    let temp = TempRoot::new();
    let contents = r#"{"schema_version":2,"data":{"theme":"light"}}"#;
    let path = seed(temp.path(), contents);
    let before = fs::read(&path).expect("before");

    let error = save_settings(temp.path(), &SettingsData::default()).expect_err("must refuse");

    assert!(matches!(
        error,
        SettingsError::RefusedSchema { found: 2, .. }
    ));
    assert_eq!(fs::read(&path).expect("after"), before);
}

#[test]
fn saving_over_an_unreadable_document_is_refused() {
    let temp = TempRoot::new();
    let path = seed(temp.path(), "{ broken");
    let before = fs::read(&path).expect("before");

    let error = save_settings(temp.path(), &SettingsData::default()).expect_err("must refuse");

    assert!(matches!(error, SettingsError::RefusedUnreadable { .. }));
    assert_eq!(fs::read(&path).expect("after"), before);
}

#[test]
fn saving_preserves_fields_written_by_a_newer_build() {
    let temp = TempRoot::new();
    seed(
        temp.path(),
        r#"{"schema_version":1,"data":{"theme":"light","future_setting":true}}"#,
    );

    let mut settings = load_settings(temp.path()).expect("load").data;
    settings.theme = ThemeChoice::Dark;
    save_settings(temp.path(), &settings).expect("save");

    let text = fs::read_to_string(temp.path().join("settings.json")).expect("read");
    assert!(text.contains("future_setting"), "{text}");
    assert!(text.contains("\"theme\": \"dark\""), "{text}");
}
