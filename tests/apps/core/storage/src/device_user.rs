use super::*;

#[test]
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn current_user_has_a_stable_digest() {
    let first = DeviceUserId::current().unwrap();
    assert_eq!(first.as_str(), DeviceUserId::current().unwrap().as_str());
    assert_eq!(first.as_str().len(), 64);
    assert!(first.as_str().bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn normalizes_existing_machine_id_formats_but_rejects_uninitialized_ids() {
    assert_eq!(
        normalize_machine_id("5E536A78-6C53-4E09-AEA6-19D971E97C9F\n").as_deref(),
        Some("5e536a786c534e09aea619d971e97c9f")
    );
    assert_eq!(
        normalize_machine_id("00000000000000000000000000000000"),
        None
    );
    assert_eq!(normalize_machine_id("uninitialized"), None);
}
