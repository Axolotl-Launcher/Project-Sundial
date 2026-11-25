// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Instance rows and their install and launch actions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axlauncher_core::desktop::installation::install_key;
use axlauncher_core::state::profile::{DiscoveredProfile, Game};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    Button, ButtonVariants, Context, Disableable, ElementId, Hsla, IntoElement, Label, StyledExt,
    div,
};

use crate::RootView;
use crate::installations::Installation;
use crate::launches::Launch;

/// What one row's buttons need to know about its profile.
struct ActionState<'a> {
    directory: PathBuf,
    game: Game,
    install: Option<Option<&'a Installation>>,
    launch: Option<&'a Launch>,
    launchable: bool,
    account_pending: bool,
    /// Whether this profile recorded its own version id. A shared install slot
    /// can be Complete from another profile, and this one still needs the
    /// button that would produce its id.
    recorded: bool,
}

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
    account_pending: bool,
    muted: Hsla,
    cx: &mut Context<'_, RootView>,
) -> impl IntoElement + use<> {
    let action = instance.profile.game().and_then(|game| {
        // A loader profile has no launchable version until its install
        // produces one, so an absent version must not hide the install action
        // that would create it.
        let version = game.effective_version();
        let install =
            install_key(game).map(|key| installations.and_then(|states| states.get(&key)));
        let launch = match (version, launch) {
            (Some(version), Some((started_version, state)))
                if state.is_active() || started_version == version =>
            {
                Some(state)
            }
            _ => None,
        };
        (install.is_some() || launch.is_some()).then(|| {
            // The install slot is keyed by what the profile asked for, so a
            // second profile naming the same game and loader shares it. Only a
            // profile that recorded its own version id has an install to show;
            // the other would hide Install behind someone else's Complete and
            // have no version to launch.
            instance_actions(
                ActionState {
                    directory: instance.directory.clone(),
                    game: game.clone(),
                    install,
                    launch,
                    launchable: version.is_some(),
                    account_pending,
                    recorded: game.version_id.is_some(),
                },
                muted,
                cx,
            )
        })
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
    let details = profile.game().map_or_else(
        || "No Minecraft version".to_string(),
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
    state: ActionState<'_>,
    muted: Hsla,
    cx: &mut Context<'_, RootView>,
) -> impl IntoElement + use<> {
    let ActionState {
        directory,
        game,
        install,
        launch,
        launchable,
        account_pending,
        recorded,
    } = state;
    let slot = install.flatten();
    // Complete is the shared slot's answer, which another profile may have
    // produced. A profile with no version id of its own still needs the button.
    let installation = match slot {
        Some(Installation::Complete) if !recorded => None,
        other => other,
    };
    let busy =
        launch.is_some_and(Launch::is_active) || matches!(slot, Some(Installation::Running(_)));
    let message = action_message(installation, launch);
    let install_button = install.and_then(|_| {
        let label = install_label(slot, recorded)?;
        let directory = directory.clone();
        let game = game.clone();
        Some(
            Button::new(ElementId::Name(
                format!("install:{}", directory.display()).into(),
            ))
            .primary()
            .disabled(busy)
            .label(label)
            .on_click(cx.listener(move |view, _, _, cx| view.install_game(&directory, &game, cx))),
        )
    });
    let buttons = div()
        .h_flex()
        .gap_2()
        .children(install_button)
        .children(launchable.then(|| launch_button(&directory, busy || account_pending, cx)));
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

/// The line under the buttons, or nothing when there is nothing to say.
fn action_message(installation: Option<&Installation>, launch: Option<&Launch>) -> Option<String> {
    match (installation, launch) {
        (_, Some(status)) if status.is_active() => Some(status.message().to_owned()),
        (Some(status @ (Installation::Running(_) | Installation::Failed(_))), _) => {
            Some(status.message())
        }
        (Some(Installation::Complete), Some(Launch::NeedsInstall(_))) => {
            Some(Installation::Complete.message())
        }
        (_, Some(status)) => Some(status.message().to_owned()),
        (installation, None) => installation.map(Installation::message),
    }
}

/// The Install button's label, or nothing when it must not be shown.
fn install_label(slot: Option<&Installation>, recorded: bool) -> Option<&'static str> {
    match slot {
        None => Some("Install"),
        Some(Installation::Failed(_)) => Some("Retry"),
        Some(Installation::Running(_)) => None,
        Some(Installation::Complete) if recorded => None,
        Some(Installation::Complete) => Some("Install"),
    }
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
