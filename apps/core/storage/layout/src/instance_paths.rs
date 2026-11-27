use std::path::{Path, PathBuf};
use std::{fs, io};

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
