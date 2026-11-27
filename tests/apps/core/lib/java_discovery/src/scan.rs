use super::*;

#[test]
fn conventional_scanning_respects_the_directory_depth_and_handles_bundles() {
    let root = tempfile::tempdir().unwrap();
    let mut expected = [
        java(&root.path().join("component/platform/runtime")),
        java(&root.path().join("temurin.jdk/Contents/Home")),
        java(
            &root
                .path()
                .join("gamma/mac-os/gamma/jre.bundle/Contents/Home"),
        ),
    ];
    java(&root.path().join("too/many/levels/deep"));

    let mut found = collect(|scan| scan.conventional_roots([root.path().to_owned()]));
    found.sort();
    expected.sort();
    assert_eq!(found, expected);
}

#[test]
fn a_lookalike_does_not_hide_a_jdk_under_an_unrecognized_vendor() {
    let root = tempfile::tempdir().unwrap();
    let vendor = root.path().join("Example Vendor");
    let lookalike = java(&vendor);
    fs::remove_file(vendor.join("release")).unwrap();
    let expected = java(&vendor.join("Product 21"));

    assert!(JavaRuntime::inspect_automatic(&lookalike).is_none());
    assert_eq!(
        collect(|scan| scan.conventional_roots([root.path().to_owned()])),
        [expected]
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_can_name_a_java_home_but_cannot_extend_the_search() {
    use std::os::unix::fs::symlink;

    let sources = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let target = sources.path().join("home");
    java(&target);
    java(&sources.path().join("container/hidden"));
    let alias = root.path().join("linked-home");
    symlink(target, &alias).unwrap();
    symlink(sources.path().join("container"), root.path().join("loop")).unwrap();

    assert_eq!(
        collect(|scan| scan.conventional_roots([root.path().to_owned()])),
        [executable_in_home(&alias, JavaMode::Console).unwrap()]
    );
}

#[test]
fn cancellation_stops_scanning_even_after_emitting_a_candidate() {
    let root = tempfile::tempdir().unwrap();
    let first = java(&root.path().join("first"));
    java(&root.path().join("second"));
    let cancellation = Cancellation::default();
    let mut found = Vec::new();
    let result = Scanner::new(&cancellation, &mut |path| {
        found.push(path);
        cancellation.cancel();
        Ok(())
    })
    .conventional_roots([root.path().join("first"), root.path().join("second")]);

    assert_eq!(found, [first]);
    assert_eq!(result.unwrap_err(), "Java discovery cancelled.");
}

pub(crate) fn collect(run: impl FnOnce(&mut Scanner<'_>) -> Result<(), String>) -> Vec<PathBuf> {
    let mut found = Vec::new();
    run(&mut Scanner::new(&Cancellation::default(), &mut |path| {
        found.push(path);
        Ok(())
    }))
    .unwrap();
    found
}

pub(crate) fn java(home: &Path) -> PathBuf {
    let bin = home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(home.join("release"), "JAVA_VERSION=\"21\"\n").unwrap();
    let path = bin.join(if cfg!(windows) { "java.exe" } else { "java" });
    fs::write(&path, b"fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}
