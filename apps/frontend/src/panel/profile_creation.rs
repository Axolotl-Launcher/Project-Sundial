// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Version selection and catalog queries for the instance creation form.

use std::{future::Future, time::Duration};

use axlauncher_core::install::vanilla::{self, MinecraftCatalog, MinecraftVersionKind};
use axlauncher_core::loader::{self, SUPPORTED_KINDS};
use axlauncher_core::network::ReqwestNetwork;
use axlauncher_core::spi::{LoaderError, LoaderKind, LoaderVersion};
use axlauncher_core::state::profile::Game;
use axlauncher_ui::component::IndexPath;
use axlauncher_ui::{
    App, AppContext, AsyncWindowContext, Context, Entity, InputEvent, InputState, SearchableVec,
    SelectEvent, SelectItem, SelectState, SharedString, WeakEntity, Window,
};

use super::RootView;

mod view;

const AUTOMATIC: &str = "";
const VANILLA: &str = "vanilla";
const CATALOG_TIMEOUT: Duration = Duration::from_secs(15);

type Picker = SelectState<SearchableVec<Choice>>;
type ChoiceEvent = SelectEvent<SearchableVec<Choice>>;

#[derive(Clone)]
struct Choice {
    id: SharedString,
    label: SharedString,
}

impl Choice {
    fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

impl SelectItem for Choice {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.id
    }
}

#[derive(Default)]
enum CatalogStatus {
    #[default]
    Idle,
    Loading,
    Ready,
    Empty,
    Failed(String),
}

#[derive(Default)]
struct LoaderCatalog {
    revision: u64,
    status: CatalogStatus,
}

impl LoaderCatalog {
    fn restart(&mut self) -> u64 {
        self.revision += 1;
        self.status = CatalogStatus::Idle;
        self.revision
    }

    fn accepts(&self, revision: u64) -> bool {
        self.revision == revision && matches!(self.status, CatalogStatus::Loading)
    }
}

pub(super) struct ProfileCreation {
    name: Entity<InputState>,
    minecraft: Entity<Picker>,
    loader: Entity<Picker>,
    loader_version: Entity<Picker>,
    manual_game: Entity<InputState>,
    manual_build: Entity<InputState>,
    manual: bool,
    minecraft_status: CatalogStatus,
    loader_catalog: LoaderCatalog,
}

impl ProfileCreation {
    pub(super) fn new(window: &mut Window, cx: &mut Context<'_, RootView>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx));
        let manual_game = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. 1.21.11"));
        let manual_build = cx.new(|cx| InputState::new(window, cx).placeholder("Automatic"));
        let minecraft = cx.new(|cx| {
            SelectState::new(SearchableVec::new(vec![]), None, window, cx).searchable(true)
        });
        let loader =
            cx.new(|cx| SelectState::new(loader_choices(), Some(IndexPath::default()), window, cx));
        let loader_version = cx.new(|cx| {
            SelectState::new(SearchableVec::new(vec![]), None, window, cx).searchable(true)
        });
        for input in [&name, &manual_build] {
            cx.subscribe(input, RootView::profile_text_changed).detach();
        }
        cx.subscribe_in(&manual_game, window, RootView::manual_game_changed)
            .detach();
        for picker in [&minecraft, &loader] {
            cx.subscribe_in(picker, window, RootView::profile_version_changed)
                .detach();
        }
        cx.subscribe(&loader_version, |_, _, _: &ChoiceEvent, cx| cx.notify())
            .detach();
        Self {
            name,
            minecraft,
            loader,
            loader_version,
            manual_game,
            manual_build,
            manual: false,
            minecraft_status: CatalogStatus::Idle,
            loader_catalog: LoaderCatalog::default(),
        }
    }

    fn loader_kind(&self, cx: &App) -> Option<LoaderKind> {
        LoaderKind::parse(self.loader.read(cx).selected_value()?.as_ref())
    }

    pub(super) fn profile(&self, cx: &App) -> Option<(String, Game)> {
        let name = self.name.read(cx).value();
        let name = nonempty(&name)?.to_owned();
        let minecraft = if self.manual {
            self.manual_game.read(cx).value()
        } else {
            self.minecraft.read(cx).selected_value()?.clone()
        };
        let game = Game::new(nonempty(&minecraft)?);
        let Some(kind) = self.loader_kind(cx) else {
            return Some((name, game));
        };
        let build = if self.manual {
            self.manual_build.read(cx).value()
        } else {
            self.loader_version.read(cx).selected_value()?.clone()
        };
        Some((
            name,
            game.with_loader(kind.as_str(), nonempty(&build).map(str::to_owned)),
        ))
    }
}

impl RootView {
    fn profile_text_changed(
        &mut self,
        _: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(event, InputEvent::Change) {
            cx.notify();
        }
    }

    pub(super) fn load_minecraft_versions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(
            self.profile_creation.minecraft_status,
            CatalogStatus::Loading
        ) {
            return;
        }
        self.profile_creation.minecraft_status = CatalogStatus::Loading;
        let request = async { vanilla::versions(&network()?).await };
        let task = cx.background_spawn(async { catalog_request(request) });
        cx.spawn_in(window, async move |view, cx| {
            complete_minecraft_request(view, cx, task.await);
        })
        .detach();
        cx.notify();
    }

    fn set_minecraft_versions(
        &mut self,
        result: Result<MinecraftCatalog, String>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let state = &mut self.profile_creation;
        state.minecraft_status = match result {
            Ok(catalog) if catalog.versions().is_empty() => CatalogStatus::Empty,
            Ok(catalog) => {
                let entered = state.manual_game.read(cx).value();
                let (items, selected) = minecraft_picker_items(&catalog, &entered);
                set_picker(&state.minecraft, items, &selected, window, cx);
                CatalogStatus::Ready
            }
            Err(error) => CatalogStatus::Failed(error),
        };
        self.load_loader_versions(window, cx);
    }

    fn profile_version_changed(
        &mut self,
        _: &Entity<Picker>,
        _: &ChoiceEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.profile_creation.manual {
            self.clear_manual_loader_build(window, cx);
            return;
        }
        self.load_loader_versions(window, cx);
    }

    fn manual_game_changed(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(event, InputEvent::Change) {
            self.clear_manual_loader_build(window, cx);
        }
    }

    fn clear_manual_loader_build(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.profile_creation
            .manual_build
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    fn load_loader_versions(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let state = &mut self.profile_creation;
        let revision = state.loader_catalog.restart();
        state.loader_version.update(cx, |picker, cx| {
            picker.set_items(SearchableVec::new(vec![]), window, cx);
        });
        cx.notify();
        let (false, Some(kind), Some(game)) = (
            state.manual,
            state.loader_kind(cx),
            state.minecraft.read(cx).selected_value().cloned(),
        ) else {
            return;
        };
        state.loader_catalog.status = CatalogStatus::Loading;
        let request = async move { fetch_loader_versions(kind, &game).await };
        let task = cx.background_spawn(async { catalog_request(request) });
        cx.spawn_in(window, async move |view, cx| {
            complete_loader_request(view, cx, revision, task.await);
        })
        .detach();
    }

    fn set_loader_versions(
        &mut self,
        revision: u64,
        result: Result<Vec<LoaderVersion>, String>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let state = &mut self.profile_creation;
        if !state.loader_catalog.accepts(revision) {
            return;
        }
        state.loader_catalog.status = match result {
            Ok(versions) if versions.is_empty() => CatalogStatus::Empty,
            Ok(versions) => {
                let automatic = SharedString::from(AUTOMATIC);
                set_picker(
                    &state.loader_version,
                    loader_build_choices(state.loader_kind(cx), versions),
                    &automatic,
                    window,
                    cx,
                );
                CatalogStatus::Ready
            }
            Err(error) => CatalogStatus::Failed(error),
        };
        cx.notify();
    }

    fn toggle_manual_versions(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let state = &mut self.profile_creation;
        state.manual = !state.manual;
        if state.manual {
            copy_selection(&state.minecraft, &state.manual_game, window, cx);
            copy_selection(&state.loader_version, &state.manual_build, window, cx);
            self.load_loader_versions(window, cx);
            return;
        }
        if !matches!(state.minecraft_status, CatalogStatus::Ready) {
            self.load_minecraft_versions(window, cx);
        }
        self.restore_manual_game(window, cx);
        self.load_loader_versions(window, cx);
    }

    fn restore_manual_game(&self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let state = &self.profile_creation;
        let game = state.manual_game.read(cx).value();
        let Some(game) = nonempty(&game) else {
            return;
        };
        let game = SharedString::from(game.to_owned());
        state.minecraft.update(cx, |picker, cx| {
            picker.set_selected_value(&game, window, cx)
        });
    }
}

fn complete_minecraft_request(
    view: WeakEntity<RootView>,
    cx: &mut AsyncWindowContext,
    result: Result<MinecraftCatalog, String>,
) {
    let _ = view.update_in(cx, |view, window, cx| {
        view.set_minecraft_versions(result, window, cx)
    });
}

fn complete_loader_request(
    view: WeakEntity<RootView>,
    cx: &mut AsyncWindowContext,
    revision: u64,
    result: Result<Vec<LoaderVersion>, String>,
) {
    let _ = view.update_in(cx, |view, window, cx| {
        view.set_loader_versions(revision, result, window, cx)
    });
}

fn set_picker(
    picker: &Entity<Picker>,
    items: SearchableVec<Choice>,
    value: &SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    picker.update(cx, |picker, cx| {
        picker.set_items(items, window, cx);
        picker.set_selected_value(value, window, cx);
    });
}

fn copy_selection(
    picker: &Entity<Picker>,
    input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(value) = picker.read(cx).selected_value().cloned() {
        input.update(cx, |input, cx| input.set_value(value, window, cx));
    }
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn minecraft_picker_items(
    catalog: &MinecraftCatalog,
    entered: &str,
) -> (SearchableVec<Choice>, SharedString) {
    let selected = nonempty(entered).unwrap_or_else(|| catalog.latest_release());
    let items = SearchableVec::new(
        catalog
            .versions()
            .iter()
            .map(|version| {
                let category = match version.kind() {
                    MinecraftVersionKind::Release => "Release",
                    MinecraftVersionKind::Snapshot => "Snapshot",
                    MinecraftVersionKind::OldBeta => "Old beta",
                    MinecraftVersionKind::OldAlpha => "Old alpha",
                    MinecraftVersionKind::Other => "Other",
                };
                Choice::new(
                    version.id().to_owned(),
                    format!("{} · {category}", version.id()),
                )
            })
            .collect::<Vec<_>>(),
    );
    (items, SharedString::from(selected.to_owned()))
}

fn loader_choices() -> SearchableVec<Choice> {
    let mut choices = SearchableVec::new(vec![Choice::new(VANILLA, "Vanilla")]);
    for kind in SUPPORTED_KINDS {
        let label = match kind {
            LoaderKind::Fabric => "Fabric",
            LoaderKind::LegacyFabric => "Legacy Fabric",
            LoaderKind::Quilt => "Quilt",
            LoaderKind::Forge => "Forge",
            LoaderKind::NeoForge => "NeoForge",
            LoaderKind::OptiFine => "OptiFine",
        };
        choices.push(Choice::new(kind.as_str(), label));
    }
    choices
}

fn loader_build_choices(
    kind: Option<LoaderKind>,
    versions: Vec<LoaderVersion>,
) -> SearchableVec<Choice> {
    let stability_label = if kind == Some(LoaderKind::Forge) {
        "Recommended"
    } else {
        "Stable"
    };
    let mut choices = SearchableVec::new(vec![Choice::new(AUTOMATIC, "Automatic")]);
    for version in versions {
        let label = if version.stable == Some(true) {
            format!("{} · {stability_label}", version.version)
        } else {
            version.version.clone()
        };
        choices.push(Choice::new(version.version, label));
    }
    choices
}

fn network() -> Result<ReqwestNetwork, String> {
    ReqwestNetwork::desktop().map_err(|error| error.to_string())
}

fn catalog_request<T>(request: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async {
        tokio::time::timeout(CATALOG_TIMEOUT, request)
            .await
            .map_err(|_| "The request timed out.".to_owned())?
    })
}

async fn fetch_loader_versions(kind: LoaderKind, game: &str) -> Result<Vec<LoaderVersion>, String> {
    match loader::versions(network()?, kind, game).await {
        Ok(versions) => Ok(versions),
        Err(LoaderError::NoVersions { .. }) => Ok(Vec::new()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/frontend/src/panel/profile_creation.rs"]
mod tests;
