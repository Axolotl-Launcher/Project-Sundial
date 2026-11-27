use std::path::{Path, PathBuf};
use std::{fs, io};

pub fn profile_document(instance: &Path) -> PathBuf {
    instance.join(".axlprofile")
}

pub fn instance_file_index(instance: &Path) -> PathBuf {
    instance.join("files.json")
}

pub fn instance_options(instance: &Path) -> PathBuf {
    instance.join("options.txt")
}

pub fn instance_mods(instance: &Path) -> PathBuf {
    instance.join("mods")
}

/// A private staging directory and its instance tree, beside the final instance.
pub fn stage_instance(instances: &Path) -> io::Result<(tempfile::TempDir, PathBuf)> {
    fs::create_dir_all(instances)?;
    let staging = tempfile::Builder::new()
        .prefix(".staging-")
        .tempdir_in(instances)?;
    let directory = staging.path().join("instance");
    fs::create_dir(&directory)?;
    Ok((staging, directory))
}
