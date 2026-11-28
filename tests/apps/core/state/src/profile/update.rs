use std::fs;

use serde_json::json;

use super::*;
use crate::{
    javas,
    profile::{self, Game, Profile},
};

fn manifest(directory: &Path) -> Value {
    serde_json::from_slice(&fs::read(Document::Profile.path(directory)).unwrap()).unwrap()
}

fn fixture(root: &Path) -> (PathBuf, String) {
    let directory = profile::create(root, "Original", Game::new("1.21.11")).unwrap();
    let id = profile::read(&directory).unwrap().unwrap().id().to_owned();
    (directory, id)
}

#[test]
fn rename_only_changes_the_exact_name_and_keeps_the_local_java_binding() {
    let root = tempfile::tempdir().unwrap();
    let (directory, id) = fixture(root.path());
    let path = Document::Profile.path(&directory);
    let mut before = manifest(&directory);
    before["id"] = id.to_uppercase().into();
    before["memory"] = json!({"maximum_mib": 15});
    before["instance"]["future"] = json!({"keep": true});
    before["game"]["future"] = json!(42);
    before["custom"] = json!(["keep"]);
    fs::write(&path, serde_json::to_vec(&before).unwrap()).unwrap();
    javas::update(root.path(), |catalog| {
        catalog.set_profile_preference(&id, Some(root.path().join("java")));
    })
    .unwrap();
    let store = Document::JavaCatalog.path(root.path());
    let catalog = fs::read(&store).unwrap();

    let renamed =
        rename_if_current(&directory, &id, "Original", "  Duplicate / 雪  ", || true).unwrap();
    assert_eq!(renamed.id(), id);
    assert_eq!(renamed.game().unwrap().effective_version(), Some("1.21.11"));
    assert_eq!(renamed.instance.name, "Duplicate / 雪");
    before["instance"]["name"] = json!("Duplicate / 雪");
    assert_eq!(manifest(&directory), before);
    assert_eq!(fs::read(store).unwrap(), catalog);
}

#[test]
fn a_heap_write_observes_the_latest_other_fields_and_rejects_stale_overrides() {
    let root = tempfile::tempdir().unwrap();
    let (directory, id) = fixture(root.path());
    let heap = MaximumHeap::new(2048).unwrap();
    rename_if_current(&directory, &id, "Original", "New name", || true).unwrap();
    let updated = set_memory_if_current(&directory, &id, None, Some(heap), || true).unwrap();
    assert_eq!(updated.memory(), Some(heap));
    assert_eq!(updated.instance.name, "New name");
    let before = manifest(&directory);
    assert!(matches!(
        set_memory_if_current(&directory, &id, None, None, || panic!("not committing")),
        Err(ProfileUpdateError::ChangedField(_))
    ));
    assert!(matches!(
        rename_if_current(&directory, &id, "Original", "Stale", || panic!(
            "not committing"
        )),
        Err(ProfileUpdateError::ChangedField(_))
    ));
    assert_eq!(manifest(&directory), before);
    set_memory_if_current(&directory, &id, Some(heap), None, || true).unwrap();
    assert!(manifest(&directory).get("memory").is_none());
}

#[test]
fn invalid_cancelled_replaced_and_unowned_profiles_are_not_written() {
    let root = tempfile::tempdir().unwrap();
    let (directory, id) = fixture(root.path());
    let path = Document::Profile.path(&directory);
    let before = fs::read(&path).unwrap();
    assert!(rename_if_current(&directory, &id, "Original", "x\ny", || true).is_err());
    assert!(matches!(
        rename_if_current(&directory, &id, "Original", "OK", || false),
        Err(ProfileUpdateError::Cancelled)
    ));
    assert_eq!(fs::read(&path).unwrap(), before);

    let replacement = Profile::new("Original", Some(Game::new("1.21.11")));
    profile::save(&directory, &replacement).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(matches!(
        rename_if_current(&directory, &id, "Original", "Wrong UUID", || true),
        Err(ProfileUpdateError::ChangedProfile(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    let mut other = manifest(&directory);
    other["schema_version"] = json!(2);
    let before = serde_json::to_vec(&other).unwrap();
    fs::write(&path, &before).unwrap();
    assert!(matches!(
        rename_if_current(
            &directory,
            replacement.id(),
            "Original",
            "Read only",
            || true
        ),
        Err(ProfileUpdateError::Read(
            DiscoveryError::UnsupportedSchema { found: 2, .. }
        ))
    ));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn an_atomic_write_error_keeps_the_original_marker() {
    let root = tempfile::tempdir().unwrap();
    let (directory, id) = fixture(root.path());
    let path = Document::Profile.path(&directory);
    let old = fs::read(&path).unwrap();
    let mut blocked = path.as_os_str().to_owned();
    blocked.push(".tmp");
    fs::create_dir(Path::new(&blocked)).unwrap();
    assert!(matches!(
        rename_if_current(&directory, &id, "Original", "Not saved", || true),
        Err(ProfileUpdateError::Write(ProfileWriteError::Write { .. }))
    ));
    assert_eq!(fs::read(path).unwrap(), old);
}
