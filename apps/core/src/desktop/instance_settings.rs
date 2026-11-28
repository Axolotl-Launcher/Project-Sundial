//! Cold edits validated by the embedded settings worker and committed by state.

use std::fmt;
use std::path::{Path, PathBuf};

use axlauncher_domain::{
    is_visible_instance_name,
    memory::{MaximumHeap, MemoryAllocation},
};
use axlauncher_state::profile::{
    self, DiscoveredProfile, Profile, ProfileFolderName, ProfileUpdateError,
};
use axlauncher_state::{self as state, SettingsError};
use axlauncher_storage::{DataDir, DataPath};
use axlauncher_worker::instance_settings::{self, Edit, InvalidEdit, Reply, ValidatedEdit};

use super::on_worker;

pub use axlauncher_worker::Cancellation as EditCancellation;
pub use axlauncher_worker::instance_settings::{GlobalHeap, InstanceHeap};

/// The instance viewed by the caller, including its stable identity and edit baselines.
pub struct InstanceTarget {
    directory: PathBuf,
    id: String,
    heap: Option<MaximumHeap>,
    name: String,
    data_root: PathBuf,
    profile_root: PathBuf,
}

impl InstanceTarget {
    pub fn new(entry: &DiscoveredProfile, data: &DataDir) -> Self {
        Self {
            directory: entry.directory.clone(),
            id: entry.profile.id().to_owned(),
            heap: entry.profile.memory(),
            name: entry.profile.instance.name.clone(),
            data_root: data.path(DataPath::Root).to_owned(),
            profile_root: data.path(DataPath::Profiles).to_owned(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    fn check_scope(&self, data: &DataDir) -> Result<(), EditError> {
        if self.data_root != data.path(DataPath::Root)
            || self.profile_root != data.path(DataPath::Profiles)
            || self.directory.parent() != Some(&self.profile_root)
            || self.directory.file_name().is_none()
        {
            return Err(EditError::ChangedScope);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum EditError {
    Worker(String),
    InvalidHeap,
    InvalidName,
    ChangedScope,
    Settings(SettingsError),
    Instance(ProfileUpdateError),
}

impl fmt::Display for EditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Worker(error) => formatter.write_str(error),
            Self::InvalidHeap => write!(
                formatter,
                "The maximum heap must be at least {} MiB.",
                MaximumHeap::MINIMUM_MIB
            ),
            Self::InvalidName => write!(formatter, "The instance name must be visible text."),
            Self::ChangedScope => write!(
                formatter,
                "This instance changed. Reopen the editor to continue."
            ),
            Self::Settings(error) => error.fmt(formatter),
            Self::Instance(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for EditError {}

/// A global edit cannot materialize a preference into any instance.
pub fn set_global_heap(
    data: &DataDir,
    expected: MemoryAllocation,
    choice: GlobalHeap,
    cancel: &EditCancellation,
) -> Result<MemoryAllocation, EditError> {
    let edit = Edit::GlobalHeap(choice);
    let ValidatedEdit::GlobalHeap(memory) = checked(data, &edit, cancel)? else {
        return Err(different_edit());
    };
    state::set_memory_if_current(data.path(DataPath::Root), expected, memory, || {
        cancel.begin_commit()
    })
    .map(|settings| settings.memory())
    .map_err(EditError::Settings)
}

/// An explicit instance heap remains explicit even when it equals the default.
pub fn set_instance_heap(
    data: &DataDir,
    instance: &InstanceTarget,
    choice: InstanceHeap,
    cancel: &EditCancellation,
) -> Result<Option<MaximumHeap>, EditError> {
    instance.check_scope(data)?;
    let edit = Edit::InstanceHeap(choice);
    let ValidatedEdit::InstanceHeap(memory) = checked(data, &edit, cancel)? else {
        return Err(different_edit());
    };
    profile::set_memory_if_current(
        instance.directory(),
        instance.id(),
        instance.heap,
        memory,
        || cancel.begin_commit(),
    )
    .map(|profile| profile.memory())
    .map_err(EditError::Instance)
}

/// Renaming edits the portable display name without changing the UUID or paths.
pub fn rename(
    data: &DataDir,
    instance: &InstanceTarget,
    name: &str,
    cancel: &EditCancellation,
) -> Result<Profile, EditError> {
    let name = checked_name(data, instance, name, cancel)?;
    profile::rename_if_current(
        instance.directory(),
        instance.id(),
        &instance.name,
        &name,
        || cancel.begin_commit(),
    )
    .map_err(EditError::Instance)
}

/// Keeps the display name and folder move in one profile writer operation.
pub fn rename_with_folder(
    data: &DataDir,
    instance: &InstanceTarget,
    name: &str,
    folder: &ProfileFolderName,
    cancel: &EditCancellation,
) -> Result<PathBuf, EditError> {
    let name = checked_name(data, instance, name, cancel)?;
    profile::rename_with_folder_if_current(
        data,
        instance.directory(),
        instance.id(),
        &instance.name,
        &name,
        folder,
        || cancel.begin_commit(),
    )
    .map_err(EditError::Instance)
}

fn checked_name(
    data: &DataDir,
    instance: &InstanceTarget,
    name: &str,
    cancel: &EditCancellation,
) -> Result<String, EditError> {
    instance.check_scope(data)?;
    let edit = Edit::Rename(name.to_owned());
    match checked(data, &edit, cancel)? {
        ValidatedEdit::Rename(name) => Ok(name),
        _ => Err(different_edit()),
    }
}

fn checked(
    data: &DataDir,
    edit: &Edit,
    cancel: &EditCancellation,
) -> Result<ValidatedEdit, EditError> {
    let reply = on_worker(instance_settings::validate(data, edit, cancel))
        .map_err(|error| EditError::Worker(error.to_string()))?
        .map_err(EditError::Worker)?;
    verify_reply(edit, reply)
}

fn verify_reply(edit: &Edit, reply: Reply) -> Result<ValidatedEdit, EditError> {
    let accepted = match reply {
        Reply::Validated(accepted) => accepted,
        Reply::Rejected(InvalidEdit::InvalidHeap) => return Err(EditError::InvalidHeap),
        Reply::Rejected(InvalidEdit::InvalidName) => return Err(EditError::InvalidName),
    };
    if matches_edit(edit, &accepted) {
        return Ok(accepted);
    }
    Err(different_edit())
}

fn different_edit() -> EditError {
    EditError::Worker("The settings worker returned a different edit.".into())
}

fn matches_edit(edit: &Edit, accepted: &ValidatedEdit) -> bool {
    match (edit, accepted) {
        (
            Edit::GlobalHeap(GlobalHeap::Automatic),
            ValidatedEdit::GlobalHeap(MemoryAllocation::Automatic),
        )
        | (Edit::InstanceHeap(InstanceHeap::Inherit), ValidatedEdit::InstanceHeap(None)) => true,
        (
            Edit::GlobalHeap(GlobalHeap::ManualMib(mib)),
            ValidatedEdit::GlobalHeap(MemoryAllocation::Manual(heap)),
        )
        | (
            Edit::InstanceHeap(InstanceHeap::ManualMib(mib)),
            ValidatedEdit::InstanceHeap(Some(heap)),
        ) => *mib == heap.mib(),
        (Edit::Rename(input), ValidatedEdit::Rename(name)) => {
            input.trim() == name && is_visible_instance_name(name)
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/src/desktop/instance_settings.rs"]
mod tests;
