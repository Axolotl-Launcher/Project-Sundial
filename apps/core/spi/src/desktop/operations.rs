// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop requests found in the first-generation frontend.
//!
//! The group and name identify a source command in the wiring inventory. UI
//! code uses the constants, and domain-specific ports can take over as their
//! requests and outcomes become available.

use super::DesktopOperation;

macro_rules! operation_groups {
    ($($module:ident => $group:literal { $($constant:ident => $name:literal,)* })*) => {
        $(
            pub mod $module {
                use super::DesktopOperation;

                $(
                    pub const $constant: DesktopOperation = DesktopOperation {
                        group: $group,
                        name: $name,
                    };
                )*

                pub const ALL: &[DesktopOperation] = &[$($constant),*];
            }
        )*

        pub(super) const GROUPS: &[&[DesktopOperation]] = &[$($module::ALL),*];
    };
}

mod application;
mod content;
mod instances;

pub use application::{
    ai, app, drop, files, fonts, mcp, multiplayer, observability, observability_mcp,
    schematic_preview, seed_map, servers, settings, shortcuts, system_accent, telemetry,
    terracotta, third_party_accounts, translation, utils,
};
pub use content::{
    cache, content_search, curseforge, datapacks, mcarchive, metadata, minecraft_skins,
    mod_translation, planet_minecraft, resources, schematics, tags, theme_pack,
};
pub use instances::{auth, import, install, instance, jre, logs, process, storage, worlds};

pub fn all() -> impl Iterator<Item = DesktopOperation> {
    [application::GROUPS, content::GROUPS, instances::GROUPS]
        .into_iter()
        .flatten()
        .flat_map(|operations| operations.iter().copied())
}
