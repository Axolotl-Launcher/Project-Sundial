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
use crate::panel::profile_version;

/// One instance row.
///
/// The labels take explicit ids: `Label::new` defaults its id to the call site,
/// so labels built in a loop would share one id and gpui's accessibility tree
/// panics on the duplicate in debug builds.
pub(crate) fn row(
    instance: &DiscoveredProfile,
    selected: bool,
    installations: Option<&HashMap<String, Installation>>,
    launch: Option<&(String, Launch)>,
    muted: Hsla,
    cx: &mut Context<'_, RootView>,
) -> impl IntoElement + use<> {
    let action = instance.profile.game.as_ref().and_then(|game| {
        let version = profile_version(game)?;
        let install = vanilla_version(game).map(|version| {
            (
                version,
                installations.and_then(|states| states.get(version)),
            )
        });
        let launch = launch.and_then(|(started_version, state)| {
            (state.is_active() || started_version == version).then_some(state)
        });
        Some(instance_actions(
            &instance.directory,
            install,
            launch,
            muted,
            cx,
        ))
    });
    let path = instance.directory.clone();
    let inspect = Button::new(ElementId::Name(
        format!("inspect:{}", path.display()).into(),
    ))
    .label("Select")
    .toggled(selected)
    .accessibility_label(format!(
        "Select {} for diagnostics",
        instance.profile.instance.name
    ))
    .on_click(cx.listener(move |view, _, _, cx| view.select_instance(path.clone(), cx)));
    div()
        .h_flex()
        .justify_between()
        .items_start()
        .gap_3()
        .child(description(instance, muted))
        .child(
            div()
                .h_flex()
                .items_start()
                .gap_2()
                .child(inspect)
                .children(action),
        )
}

fn description(instance: &DiscoveredProfile, muted: Hsla) -> impl IntoElement + use<> {
    let profile = &instance.profile;
    let details = profile.game.as_ref().map_or_else(
        || "unknown version".to_string(),
        |game| match &game.loader {
            Some(loader) => format!("{} · {}", game.minecraft, loader.kind),
            None => game.minecraft.clone(),
        },
    );
    let directory = instance.directory.display();

    div()
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
        )
}

fn instance_actions(
    directory: &Path,
    install: Option<(&str, Option<&Installation>)>,
    launch: Option<&Launch>,
    muted: Hsla,
    cx: &mut Context<'_, RootView>,
) -> impl IntoElement + use<> {
    let installation = install.and_then(|(_, state)| state);
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
    let install_button = install.and_then(|(version, state)| {
        let label = match state {
            None => "Install",
            Some(Installation::Failed(_)) => "Retry",
            Some(Installation::Running(_) | Installation::Complete) => return None,
        };
        let version = version.to_owned();
        Some(
            Button::new(ElementId::Name(
                format!("install:{}", directory.display()).into(),
            ))
            .primary()
            .disabled(busy)
            .label(label)
            .on_click(cx.listener(move |view, _, _, cx| view.install(&version, cx))),
        )
    });
    let buttons = div()
        .h_flex()
        .gap_2()
        .children(install_button)
        .child(launch_button(directory, busy, cx));
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

fn launch_button(directory: &Path, busy: bool, cx: &mut Context<'_, RootView>) -> Button {
    let directory = directory.to_path_buf();
    Button::new(ElementId::Name(
        format!("launch:{}", directory.display()).into(),
    ))
    .label("Launch")
    .disabled(busy)
    .on_click(cx.listener(move |view, _, _, cx| view.launch(&directory, cx)))
}
