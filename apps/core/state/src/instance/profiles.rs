// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Discovery and numbering across the launcher's profile directories.

use std::collections::HashSet;
use std::path::Path;

use super::{
    DiscoveryError, Instance, InstanceError, create, discover, discover_in, sort_instances,
};

/// Profile discovery includes manifests saved by earlier builds of this launcher.
pub fn discover_profiles(
    data_root: &Path,
    profiles: &Path,
) -> Result<Vec<Instance>, DiscoveryError> {
    let mut instances = discover(profiles)?;
    let mut ids: HashSet<_> = instances
        .iter()
        .map(|instance| instance.id.clone())
        .collect();
    instances.extend(
        discover(&data_root.join("instances"))?
            .into_iter()
            .chain(discover_in(data_root, |id| {
                id.strip_prefix("instance-")
                    .is_some_and(|number| number.parse::<u32>().is_ok())
            })?)
            .filter(|instance| ids.insert(instance.id.clone())),
    );
    sort_instances(&mut instances);
    Ok(instances)
}

/// Instance numbers stay reserved across the current and earlier layouts.
pub fn create_numbered(profiles: &Path, existing: &[Instance]) -> Result<(), InstanceError> {
    const MAX_ATTEMPTS: u32 = 100;

    for number in 1..=MAX_ATTEMPTS {
        let id = format!("instance-{number}");
        if existing.iter().any(|instance| instance.id == id) {
            continue;
        }
        let instance = Instance::new(id, format!("Instance {number}"));
        match create(profiles, &instance) {
            Err(InstanceError::AlreadyExists { .. }) => continue,
            result => return result,
        }
    }
    Err(InstanceError::NoAvailableNumber)
}
