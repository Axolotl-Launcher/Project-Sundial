// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Instance rows and their install and launch actions.

use std::collections::HashMap;
use std::path::Path;

use axlauncher_core::desktop::installation::install_key;
use axlauncher_core::state::profile::DiscoveredProfile;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, App, Button, ButtonVariants, Disableable, Div, ElementId, IntoElement, Label,
    StyledExt, div,
};

use crate::i18n::{language, tr};

use crate::installations::Installation;
use crate::launches::Launch;

pub(super) enum RowAction {
    Select,
    Install,
    Launch,
}

/// What one row's buttons need to know about its profile.
struct ActionState<'a> {
    directory: &'a Path,
    install: Option<Option<&'a Installation>>,
    launch: Option<&'a Launch>,
    launchable: bool,
    launch_pending: bool,
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
pub(super) fn row(
    instance: &DiscoveredProfile,
    selected: bool,
    installations: Option<&HashMap<String, Installation>>,
    launch: Option<&(String, Launch)>,
    launch_pending: bool,
    cx: &App,
    on_action: impl Fn(RowAction, &mut App) + Clone + 'static,
) -> Div {
    let action = instance.profile.game().and_then(|game| {
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
        (install.is_some() || launch.is_some() || version.is_some()).then(|| {
            instance_actions(
                ActionState {
                    directory: &instance.directory,
                    install,
                    launch,
                    launchable: version.is_some(),
                    launch_pending,
                    recorded: game.version_id.is_some(),
                },
                cx,
                on_action.clone(),
            )
        })
    });
    let path = &instance.directory;
    let select = Button::new(ElementId::Name(
        format!("inspect:{}", path.display()).into(),
    ))
    .label(tr(cx, "instances.select"))
    .toggled(selected)
    .accessibility_label(crate::i18n::interpolate(
        tr(cx, "instances.select-name"),
        &[("name", &instance.profile.instance.name)],
    ))
    .on_click(move |_, _, cx| on_action(RowAction::Select, cx));
    div()
        .h_flex()
        .justify_between()
        .items_start()
        .gap_3()
        .child(description(instance, cx))
        .child(
            div()
                .h_flex()
                .items_start()
                .gap_2()
                .child(select)
                .children(action),
        )
}

fn description(instance: &DiscoveredProfile, cx: &App) -> impl IntoElement + use<> {
    let profile = &instance.profile;
    let details = profile.game().map_or_else(
        || tr(cx, "instances.no-version").to_string(),
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
            .text_color(cx.theme().muted_foreground),
        )
}

fn instance_actions(
    state: ActionState<'_>,
    cx: &App,
    on_action: impl Fn(RowAction, &mut App) + Clone + 'static,
) -> Div {
    let ActionState {
        directory,
        install,
        launch,
        launchable,
        launch_pending,
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
    let message = action_message(installation, launch, cx);
    let install_button = install.and_then(|_| {
        let label = install_label(slot, recorded, cx)?;
        let on_action = on_action.clone();
        Some(
            Button::new(ElementId::Name(
                format!("install:{}", directory.display()).into(),
            ))
            .primary()
            .disabled(busy)
            .label(label)
            .on_click(move |_, _, cx| on_action(RowAction::Install, cx)),
        )
    });
    let buttons = div().h_flex().gap_2().children(install_button).children(
        launchable.then(|| launch_button(directory, busy || launch_pending, on_action, cx)),
    );
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
                    .text_color(cx.theme().muted_foreground),
            )
        })
}

/// The line under the buttons, or nothing when there is nothing to say.
fn action_message(
    installation: Option<&Installation>,
    launch: Option<&Launch>,
    cx: &App,
) -> Option<String> {
    match (installation, launch) {
        (_, Some(status)) if status.is_active() => Some(status.message(language(cx)).to_owned()),
        (Some(status @ (Installation::Running(_) | Installation::Failed(_))), _) => {
            Some(status.message(cx))
        }
        (Some(Installation::Complete), Some(Launch::NeedsInstall(_))) => {
            Some(Installation::Complete.message(cx))
        }
        (_, Some(status)) => Some(status.message(language(cx)).to_owned()),
        (installation, None) => installation.map(|status| status.message(cx)),
    }
}

/// The Install button's label, or nothing when it must not be shown.
fn install_label(slot: Option<&Installation>, recorded: bool, cx: &App) -> Option<&'static str> {
    match slot {
        None => Some(tr(cx, "instances.install")),
        Some(Installation::Failed(_)) => Some(tr(cx, "instances.install.retry")),
        Some(Installation::Running(_)) => None,
        Some(Installation::Complete) if recorded => None,
        Some(Installation::Complete) => Some(tr(cx, "instances.install")),
    }
}

fn launch_button(
    directory: &Path,
    busy: bool,
    on_action: impl Fn(RowAction, &mut App) + 'static,
    cx: &App,
) -> Button {
    Button::new(ElementId::Name(
        format!("launch:{}", directory.display()).into(),
    ))
    .label(tr(cx, "instances.launch"))
    .disabled(busy)
    .on_click(move |_, _, cx| on_action(RowAction::Launch, cx))
}
