use super::*;

#[test]
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn current_user_has_a_stable_digest() {
    let config = tempfile::tempdir().unwrap();
    let current = || {
        DeviceUserId::resolve(
            process_principal().as_deref(),
            machine_id().as_deref(),
            Some(config.path()),
        )
        .unwrap()
    };
    let first = current();
    assert_eq!(first.as_str(), current().as_str());
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

#[test]
fn failed_native_probes_reuse_one_random_namespace_across_restarts() {
    let config = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let first = DeviceUserId::resolve(None, None, Some(config.path())).unwrap();
    let reopened = DeviceUserId::resolve(None, None, Some(config.path())).unwrap();
    let unrelated = DeviceUserId::resolve(None, None, Some(other.path())).unwrap();
    assert!(is_digest(first.as_str()));
    assert_eq!(first.as_str(), reopened.as_str());
    assert_ne!(first.as_str(), unrelated.as_str());
    assert!(
        Document::DeviceUser
            .path(&config.path().join("unknown"))
            .is_file()
    );
}

#[test]
fn a_saved_native_namespace_survives_a_failed_machine_probe() {
    let config = tempfile::tempdir().unwrap();
    let principal = "test-principal";
    let machine = "00112233445566778899aabbccddeeff";
    let expected = hex::encode(Sha256::digest(format!(
        "axlauncher:device-user:v1\0{}\0{machine}\0{principal}",
        std::env::consts::OS
    )));
    let known = DeviceUserId::resolve(Some(principal), Some(machine), Some(config.path())).unwrap();
    let missing = DeviceUserId::resolve(Some(principal), None, Some(config.path())).unwrap();
    assert_eq!(known.as_str(), expected);
    assert_eq!(missing.as_str(), expected);
}

#[test]
fn fallback_binds_on_recovery_without_changing_the_namespace_or_crossing_principals() {
    let config = tempfile::tempdir().unwrap();
    let config = Some(config.path());
    let principal = Some("first-user");
    let machine = Some("00112233445566778899aabbccddeeff");
    let fallback = DeviceUserId::resolve(principal, None, config).unwrap();
    let recovered = DeviceUserId::resolve(principal, machine, config).unwrap();
    let reopened = DeviceUserId::resolve(principal, None, config).unwrap();
    let other_principal = DeviceUserId::resolve(Some("second-user"), None, config).unwrap();
    let other_machine = DeviceUserId::resolve(principal, Some("ffee"), config).unwrap();
    assert_eq!(fallback.as_str(), recovered.as_str());
    assert_eq!(fallback.as_str(), reopened.as_str());
    assert_ne!(fallback.as_str(), other_principal.as_str());
    assert_ne!(fallback.as_str(), other_machine.as_str());
}

#[test]
fn simultaneous_first_workers_agree_on_the_installation_namespace() {
    let config = tempfile::tempdir().unwrap();
    std::thread::scope(|scope| {
        let resolve = || DeviceUserId::resolve(None, None, Some(config.path())).unwrap();
        let first = scope.spawn(resolve);
        let second = scope.spawn(resolve);
        assert_eq!(
            first.join().unwrap().as_str(),
            second.join().unwrap().as_str()
        );
    });
}

#[test]
fn an_unavailable_config_does_not_disable_a_valid_native_identity() {
    let native = DeviceUserId::resolve(Some("user"), Some("machine"), None).unwrap();
    assert!(is_digest(native.as_str()));
    assert!(DeviceUserId::resolve(None, None, None).is_err());
}
