//! Workspace-private wire values for one settings edit.

use axlauncher_domain::memory::{MaximumHeap, MemoryAllocation};
use serde::{Deserialize, Serialize};

pub use crate::settings_client::validate;

#[derive(Debug, Deserialize, Serialize)]
pub enum Edit {
    GlobalHeap(GlobalHeap),
    InstanceHeap(InstanceHeap),
    Rename(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum GlobalHeap {
    Automatic,
    ManualMib(u32),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum InstanceHeap {
    Inherit,
    ManualMib(u32),
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum ValidatedEdit {
    GlobalHeap(MemoryAllocation),
    InstanceHeap(Option<MaximumHeap>),
    Rename(String),
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum InvalidEdit {
    InvalidHeap,
    InvalidName,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum Reply {
    Validated(ValidatedEdit),
    Rejected(InvalidEdit),
}
