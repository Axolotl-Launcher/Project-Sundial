// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Instance rows and their install and launch actions.

use std::collections::HashMap;
use std::path::Path;

use axlauncher_core::state::profile::DiscoveredProfile;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    Button, ButtonVariants, Context, Disableable, ElementId, Hsla, IntoElement, Label, StyledExt,
    div,
};

use crate::RootView;
use crate::installations::{Installation, vanilla_version};
use crate::launches::Launch;

/// One instance row.
///
/// The labels take explicit ids: `Label::new` defaults its id to the call site,
/// so labels built in a loop would share one id and gpui's accessibility tree
/// panics on the duplicate in debug builds.
pub(crate) fn row(
    instance: &DiscoveredProfile,
    installations: &HashMap<String, Installation>,
    launch: Option<&Launch>,
    muted: Hsla,
    cx: &mut Context<'_, RootView>,
) -> impl IntoElement + use<> {
    let profile = &instance.profile;
    let details = profile.game.as_ref().map_or_else(
        || "unknown version".to_string(),
        |game| match &game.loader {
            Some(loader) => format!("{} · {}", game.minecraft, loader.kind),
            None => game.minecraft.clone(),
        },
    );
    let directory = instance.directory.display();

    let description = div()
        .v_flex()
        .gap_1()
        .child(
            Label::new(profile.instance.name.clone())
                .id(ElementId::Name(format!("instance-name:{directory}").into()))
                .text_base(),
        )
        .child(
            Label::new(format!(
                "{} · {details}",
                instance
                    .directory
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ))
            .id(ElementId::Name(
                format!("instance-detail:{directory}").into(),
            ))
            .text_xs()
            .text_color(muted),
        );
    let action = profile
        .game
        .as_ref()
        .and_then(vanilla_version)
        .map(|version| {
            instance_actions(
                &instance.directory,
                version,
                installations.get(version),
                launch,
                muted,
                cx,
            )
        });
    div()
        .h_flex()
        .justify_between()
        .items_start()
        .gap_3()
        .child(description)
        .when_some(action, |row, action| row.child(action))
}

fn instance_actions(
    directory: &Path,
    version: &str,
    installation: Option<&Installation>,
    launch: Option<&Launch>,
    muted: Hsla,
    cx: &mut Context<'_, RootView>,
) -> impl IntoElement + use<> {
    let busy = launch.is_some_and(Launch::is_active)
        || matches!(installation, Some(Installation::Running(_)));
    let message = match (installation, launch) {
        (_, Some(state)) if state.is_active() => Some(state.message().to_owned()),
        (Some(state @ (Installation::Running(_) | Installation::Failed(_))), _) => {
            Some(state.message())
        }
        (Some(Installation::Complete), Some(Launch::NeedsInstall(_))) => {
            Some(Installation::Complete.message())
        }
        (_, Some(launch)) => Some(launch.message().to_owned()),
        (installation, None) => installation.map(Installation::message),
    };
    let label = match installation {
        None => Some("Install"),
        Some(Installation::Failed(_)) => Some("Retry"),
        Some(Installation::Running(_) | Installation::Complete) => None,
    };
    let buttons = div()
        .h_flex()
        .gap_2()
        .when_some(label, |action, label| {
            let version = version.to_owned();
            action.child(
                Button::new(ElementId::Name(
                    format!("install:{}", directory.display()).into(),
                ))
                .primary()
                .disabled(busy)
                .label(label)
                .on_click(cx.listener(move |view, _, _, cx| view.install(&version, cx))),
            )
        })
        .child(launch_button(directory, version, busy, cx));
    div()
        .v_flex()
        .gap_1()
        .max_w_full()
        .child(buttons)
        .when_some(message, |action, message| {
            action.child(
                Label::new(message)
                    .id(ElementId::Name(
                        format!("install-status:{}", directory.display()).into(),
                    ))
                    .text_xs()
                    .text_color(muted),
            )
        })
}

fn launch_button(
    directory: &Path,
    version: &str,
    busy: bool,
    cx: &mut Context<'_, RootView>,
) -> Button {
    let directory = directory.to_path_buf();
    let version = version.to_owned();
    Button::new(ElementId::Name(
        format!("launch:{}", directory.display()).into(),
    ))
    .label("Launch")
    .disabled(busy)
    .on_click(
        cx.listener(move |view, _, _, cx| view.launch(directory.clone(), version.clone(), cx)),
    )
}
