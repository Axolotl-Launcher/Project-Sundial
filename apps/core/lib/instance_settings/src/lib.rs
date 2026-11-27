//! Validation of edits submitted to the instance settings worker.

use axlauncher_domain::{
    is_visible_instance_name,
    memory::{MaximumHeap, MemoryAllocation},
};
use axlauncher_worker::instance_settings::{
    Edit, GlobalHeap, InstanceHeap, InvalidEdit, ValidatedEdit,
};

mod worker;

/// Validates one edit without touching launcher documents or paths.
pub fn validate(edit: Edit) -> Result<ValidatedEdit, InvalidEdit> {
    Ok(match edit {
        Edit::GlobalHeap(GlobalHeap::Automatic) => {
            ValidatedEdit::GlobalHeap(MemoryAllocation::Automatic)
        }
        Edit::GlobalHeap(GlobalHeap::ManualMib(mib)) => {
            ValidatedEdit::GlobalHeap(MemoryAllocation::Manual(heap(mib)?))
        }
        Edit::InstanceHeap(InstanceHeap::Inherit) => ValidatedEdit::InstanceHeap(None),
        Edit::InstanceHeap(InstanceHeap::ManualMib(mib)) => {
            ValidatedEdit::InstanceHeap(Some(heap(mib)?))
        }
        Edit::Rename(name) => {
            let name = name.trim();
            if !is_visible_instance_name(name) {
                return Err(InvalidEdit::InvalidName);
            }
            ValidatedEdit::Rename(name.to_owned())
        }
    })
}

fn heap(mib: u32) -> Result<MaximumHeap, InvalidEdit> {
    MaximumHeap::new(mib).ok_or(InvalidEdit::InvalidHeap)
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/instance_settings/src/lib.rs"]
mod tests;
