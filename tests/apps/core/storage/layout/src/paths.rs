use super::*;

#[test]
fn platform_defaults_keep_data_cache_and_temporary_roots_separate() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let cache = root.path().join("cache");
    let temp = root.path().join("temp");
    let paths = DataDir::resolve(
        Some(OsString::new()),
        || Some(data.clone()),
        || Some(cache.clone()),
        || temp.clone(),
    )
    .unwrap();

    assert_eq!(paths.path(DataPath::Root), data.join("AXLauncher"));
    assert_eq!(
        paths.path(DataPath::Profiles),
        data.join("AXLauncher/profiles")
    );
    assert_eq!(paths.path(DataPath::Cache), cache.join("AXLauncher"));
    assert_eq!(paths.path(DataPath::Temp), temp);
}

#[test]
fn an_environment_override_isolates_all_roots_without_using_platform_defaults() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path();
    let paths = DataDir::resolve(
        Some(path.into()),
        || panic!("must not read system data"),
        || panic!("must not read system cache"),
        || panic!("must not read system temporary directory"),
    )
    .unwrap();

    assert_eq!(paths.path(DataPath::Root), path);
    assert_eq!(paths.path(DataPath::Cache), path.join("caches"));
    assert_eq!(paths.path(DataPath::Profiles), path.join("profiles"));
    assert_eq!(paths.path(DataPath::Temp), path.join("tmp"));
}

#[test]
fn session_overrides_do_not_change_snapshots_already_held_by_workers() {
    let root = tempfile::tempdir().unwrap();
    let mut paths = DataDir::isolated(root.path()).unwrap();
    let worker_paths = paths.clone();
    let other = root.path().join("other");
    paths
        .set_directory(Directory::AppData, Some(&other))
        .unwrap();
    for (kind, name) in [
        (Directory::AppCache, "custom-cache"),
        (Directory::UserData, "custom-profiles"),
        (Directory::AppTemp, "custom-temp"),
    ] {
        paths.set_directory(kind, Some(&other.join(name))).unwrap();
    }
    assert_eq!(paths.path(DataPath::Root), other);
    assert_eq!(
        paths.path(DataPath::Profiles),
        other.join("custom-profiles")
    );
    assert_eq!(paths.path(DataPath::Cache), other.join("custom-cache"));
    assert_eq!(paths.path(DataPath::Temp), other.join("custom-temp"));
    assert!(!other.exists());
    assert_eq!(
        worker_paths.path(DataPath::Cache),
        root.path().join("caches")
    );

    for kind in [
        Directory::AppData,
        Directory::AppCache,
        Directory::UserData,
        Directory::AppTemp,
    ] {
        paths.set_directory(kind, None).unwrap();
    }
    for location in [
        DataPath::Root,
        DataPath::Cache,
        DataPath::Profiles,
        DataPath::Temp,
        DataPath::JavaVersions,
        DataPath::DownloadProbe,
        DataPath::AuthAgent,
    ] {
        assert_eq!(paths.path(location), worker_paths.path(location));
    }
}

#[test]
fn invalid_override_does_not_replace_the_current_directory() {
    let root = tempfile::tempdir().unwrap();
    let file = tempfile::NamedTempFile::new_in(root.path()).unwrap();
    let mut paths = DataDir::isolated(root.path()).unwrap();
    let error = paths
        .set_directory(Directory::AppData, Some(file.path()))
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotADirectory);
    assert!(
        paths
            .set_directory(Directory::AppData, Some(Path::new("")))
            .is_err()
    );
    assert_eq!(paths.path(DataPath::Root), root.path());
}

#[test]
fn scratch_obeys_the_override_and_cleans_up_only_its_own_directory() {
    let root = tempfile::tempdir().unwrap();
    let mut paths = DataDir::isolated(root.path()).unwrap();
    let scratch_root = root.path().join("scratch");
    paths
        .set_directory(Directory::AppTemp, Some(&scratch_root))
        .unwrap();
    let scratch = paths.scratch(ScratchSpace::Task).unwrap();
    let path = scratch.path().to_path_buf();
    assert_eq!(path.parent(), Some(scratch_root.as_path()));
    assert!(path.is_dir());
    drop(scratch);
    assert!(!path.exists());
    assert!(scratch_root.is_dir());
}

#[test]
fn worker_scratch_uses_the_cache_override_and_not_general_temporary_space() {
    let root = tempfile::tempdir().unwrap();
    let mut paths = DataDir::isolated(root.path()).unwrap();
    let cache = root.path().join("custom cache");
    let temp = root.path().join("custom temp");
    paths
        .set_directory(Directory::AppCache, Some(&cache))
        .unwrap();
    paths
        .set_directory(Directory::AppTemp, Some(&temp))
        .unwrap();

    let scratch = paths.scratch(ScratchSpace::Worker).unwrap();
    let path = scratch.path().to_path_buf();
    assert_eq!(path.parent(), Some(cache.join("workers").as_path()));
    assert!(!temp.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o700);
    }
    drop(scratch);
    assert!(!path.exists());
}
