use std::fs::{self, File};
use std::io;
use std::path::PathBuf;

use crate::{GameDir, VersionPath};

/// Prevents local launch from accepting the outputs of an interrupted processor pass.
pub struct ProcessorMarker(PathBuf);

impl ProcessorMarker {
    /// The named version must already have passed the caller's version-ID validation.
    pub fn for_version(game: &GameDir, id: &str) -> Self {
        Self(game.version_path(id, VersionPath::ProcessorIncomplete))
    }

    pub fn is_incomplete(&self) -> io::Result<bool> {
        match fs::symlink_metadata(&self.0) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// The install lock must cover both this write and the processor pass it announces.
    pub fn begin(&self) -> io::Result<()> {
        match File::create_new(&self.0) {
            Ok(file) => file.sync_all()?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error),
        }
        #[cfg(unix)]
        File::open(self.0.parent().expect("version directory"))?.sync_all()?;
        Ok(())
    }

    pub fn complete(&self) -> io::Result<()> {
        fs::remove_file(&self.0)
    }
}
