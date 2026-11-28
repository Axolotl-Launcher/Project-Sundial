//! Resolved launcher directories, managed file locations and their locks.

#![forbid(unsafe_code)]

mod documents;
mod file_lock;
mod game_dir;
mod instance_paths;
mod paths;

pub use documents::{Document, ManagedFile};
pub use file_lock::LockError;
pub use game_dir::{AssetPath, GameAccess, GameDir, VersionPath};
pub use instance_paths::{PROFILE_LOCK_PREFIX, STAGING_PREFIX, stage_instance};
pub use paths::{DataDir, DataPath, Directory, ScratchSpace, WorkerLibrary};
pub mod processor_marker;
