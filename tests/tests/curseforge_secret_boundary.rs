//! Prevents credential loading in workers and propagation through the UI.

use std::fs;
use std::path::Path;

#[test]
fn workers_do_not_load_curseforge_credentials() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for_rust_file(&root.join("apps/core/lib"), &|path, source| {
        assert!(
            !source.contains("\".env"),
            "worker reads a runtime manifest: {path:?}"
        );
        assert!(
            !source.contains("CURSEFORGE_API_KEY"),
            "worker accesses the process or build-time key: {path:?}"
        );
    });
}

#[test]
fn frontend_and_plugin_do_not_receive_or_resolve_the_key() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for directory in ["apps/frontend/src", "apps/plugins/modpack"] {
        for_rust_file(&root.join(directory), &assert_credential_free);
    }
    let operations = include_str!("../../apps/core/spi/src/desktop/operations/content.rs");
    assert!(!operations.contains("curseforge_set_api_key"));
    assert!(!operations.contains("curseforge_api_key_override"));
}

fn assert_credential_free(path: &Path, source: &str) {
    for banned in ["CURSEFORGE_API_KEY", "ApiKey", "cdn_key", "download_key"] {
        assert!(!source.contains(banned), "credential path in {path:?}");
    }
}

fn for_rust_file(root: &Path, verify: &impl Fn(&Path, &str)) {
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            for_rust_file(&path, verify);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            verify(&path, &fs::read_to_string(&path).unwrap());
        }
    }
}
