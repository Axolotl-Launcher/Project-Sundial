use syn::UseTree;

const EXCEPTIONS: &[(&str, &str, &[&str])] = &[
    (
        "apps/core/install/src/runtime.rs",
        "executable_in",
        &["bin", "JAVA_BIN"],
    ),
    (
        "apps/core/install/src/runtime.rs",
        "home_of",
        &["jre.bundle/Contents/Home"],
    ),
    ("apps/core/launch/src/java/mod.rs", "inventory", &["bin"]),
    (
        "apps/core/launch/src/java/runtime.rs",
        "read_release",
        &["release"],
    ),
    (
        "apps/core/launch/src/platform.rs",
        "executable_in_home",
        &["bin"],
    ),
    (
        "apps/core/launch/src/platform.rs",
        "managed_home",
        &["jre.bundle"],
    ),
    (
        "apps/core/launch/src/platform.rs",
        "bundle_home",
        &["Contents", "Home"],
    ),
    (
        "apps/core/state/src/profile/resources.rs",
        "scan_worlds",
        &["saves", "saves/{name}", "level.dat"],
    ),
    (
        "apps/core/state/src/profile/resources.rs",
        "collect_entry",
        &["{folder}/{name}"],
    ),
    ("apps/plugins/modpack/build.rs", "main", &["../../../.env"]),
    (
        "apps/core/lib/src/bootstrap.rs",
        "worker_executable",
        &["/proc/self/exe"],
    ),
    ("tools/mmc-convert/src/jars.rs", "add", &["layer-{}.jar"]),
    (
        "tools/mmc-convert/src/jars.rs",
        "install",
        &["replacement.jar"],
    ),
];

pub(super) fn allowed(path: &str, function: Option<&str>, literal: &str) -> bool {
    EXCEPTIONS.iter().any(|(file, name, values)| {
        *file == path && Some(*name) == function && values.contains(&literal)
    })
}

pub(super) fn imports_directory_api_glob(tree: &UseTree, directory_api: bool) -> bool {
    match tree {
        UseTree::Path(path) => {
            let directory_api = directory_api
                || matches!(
                    path.ident.to_string().as_str(),
                    "env" | "dirs" | "dirs_next" | "directories" | "directories_next" | "tempfile"
                );
            imports_directory_api_glob(&path.tree, directory_api)
        }
        UseTree::Group(group) => group
            .items
            .iter()
            .any(|tree| imports_directory_api_glob(tree, directory_api)),
        UseTree::Glob(_) => directory_api,
        _ => false,
    }
}

pub(super) fn chooses_system_directory(path: &[&str]) -> bool {
    matches!(
        path,
        [
            "dirs" | "dirs_next" | "directories" | "directories_next",
            ..
        ] | [
            ..,
            "env",
            "temp_dir" | "home_dir" | "current_dir" | "set_current_dir" | "vars" | "vars_os"
        ] | [
            "tempfile",
            "tempdir" | "tempfile" | "tempfile_in_current_dir"
        ] | [.., "NamedTempFile", "new"]
    )
}

pub(super) fn reads_environment(path: &[&str]) -> bool {
    matches!(
        path,
        [.., "env", "var" | "var_os" | "set_var" | "remove_var"]
    )
}

pub(super) fn directory_key(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    (key.starts_with("AXLAUNCHER_") && key.ends_with("_DIR"))
        || matches!(
            key.as_str(),
            "HOME"
                | "USERPROFILE"
                | "HOMEDRIVE"
                | "HOMEPATH"
                | "APPDATA"
                | "LOCALAPPDATA"
                | "XDG_DATA_HOME"
                | "XDG_CONFIG_HOME"
                | "XDG_CACHE_HOME"
                | "XDG_RUNTIME_DIR"
                | "TEMP"
                | "TMP"
                | "TMPDIR"
        )
}

pub(super) fn filesystem_arguments(path: &[&str]) -> &'static [usize] {
    match path {
        [.., "fs", "rename" | "copy" | "hard_link"] => &[0, 1],
        [
            ..,
            "Path" | "PathBuf",
            "join" | "push" | "with_file_name" | "with_extension" | "set_file_name"
            | "set_extension",
        ] => &[0, 1],
        [
            ..,
            "Path" | "PathBuf",
            "new" | "from" | "exists" | "is_file" | "is_dir",
        ]
        | [.., "File", "open" | "create" | "create_new"]
        | [.., "path", "absolute"]
        | [
            ..,
            "fs",
            "canonicalize" | "create_dir" | "create_dir_all" | "read" | "read_dir"
            | "read_to_string" | "write" | "open" | "remove_dir" | "remove_dir_all" | "remove_file"
            | "metadata" | "symlink_metadata" | "read_link" | "set_permissions",
        ] => &[0],
        _ => &[],
    }
}
