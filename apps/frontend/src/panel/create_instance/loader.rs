// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The loader and loader-version pickers, and the provider fetch behind them.
//!
//! The version list is refetched whenever the Minecraft version or the loader
//! kind changes. A fetch runs on a background worker and a reply that a newer
//! request replaced is discarded, so the shown list always belongs to the
//! game named in the form.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axlauncher_core::network::ReqwestNetwork;
use axlauncher_core::spi::{FetchError, LoaderError, LoaderKind, LoaderVersion};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, AsyncWindowContext, Context, Entity, IndexPath, IntoElement, Label,
    SearchableVec, Select, SelectEvent, SelectState, WeakEntity, Window, h_flex, v_flex,
};

use super::super::RootView;
use super::providers;

mod choice;

pub(crate) use choice::{
    LoadState, LoaderChoice, VersionItem, choices, game_for, kind_label, version_items,
};

/// How long a Minecraft version edit rests before its listing is fetched, so
/// typing one version is one request rather than one per keystroke.
pub(super) const DEBOUNCE: Duration = Duration::from_millis(350);

/// The loader choice and the version choice it feeds.
pub(super) struct LoaderPicker {
    kind: Entity<SelectState<SearchableVec<LoaderChoice>>>,
    version: Entity<SelectState<SearchableVec<VersionItem>>>,
    versions: Vec<LoaderVersion>,
    state: LoadState,
    /// Bumped for every request; a reply from an older one is discarded.
    generation: Arc<AtomicU64>,
    asked_game: String,
}

impl LoaderPicker {
    pub(super) fn new(window: &mut Window, cx: &mut Context<'_, RootView>) -> Self {
        let kind = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(choices()),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        let version = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<VersionItem>::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });
        cx.subscribe_in(&kind, window, RootView::loader_kind_changed)
            .detach();
        Self {
            kind,
            version,
            versions: Vec::new(),
            state: LoadState::Idle,
            generation: Arc::new(AtomicU64::new(0)),
            asked_game: String::new(),
        }
    }

    pub(super) fn selection(&self, cx: &Context<'_, RootView>) -> Option<LoaderChoice> {
        self.kind.read(cx).selected_value().copied()
    }

    /// The version the dropdown holds, as the provider spells it.
    pub(super) fn chosen(&self, cx: &Context<'_, RootView>) -> Option<String> {
        self.version.read(cx).selected_value().cloned()
    }

    pub(super) fn versions(&self) -> &[LoaderVersion] {
        &self.versions
    }

    /// Why a loader profile cannot be created yet, if it cannot.
    ///
    /// Vanilla has no blocker. A loader needs a game version and a finished
    /// listing; the message names whichever is missing.
    pub(super) fn blocker(&self, cx: &Context<'_, RootView>) -> Option<String> {
        let kind = self.selection(cx)?.loader()?;
        let label = kind_label(kind);
        match &self.state {
            LoadState::Ready => None,
            LoadState::Idle => Some(format!(
                "Enter a Minecraft version to list its {label} builds."
            )),
            LoadState::Loading => Some(format!("Waiting for the {label} version list.")),
            LoadState::Empty(message) | LoadState::Failed(message) => Some(message.clone()),
        }
    }

    /// Forgets the current listing; an in-flight reply is discarded.
    pub(super) fn reset(&mut self, window: &mut Window, cx: &mut Context<'_, RootView>) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.versions.clear();
        self.asked_game.clear();
        self.state = LoadState::Idle;
        self.clear_items(window, cx);
    }

    /// Asks the selected loader's provider for the builds of `game`.
    ///
    /// `delay` lets a text edit settle first; a reply from a request a newer
    /// one replaced is dropped.
    pub(super) fn refetch(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, RootView>,
        game: String,
        delay: Duration,
    ) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.versions.clear();
        self.asked_game = game.clone();
        let kind = self.selection(cx).and_then(LoaderChoice::loader);
        let Some(kind) = kind.filter(|_| !game.is_empty()) else {
            self.state = LoadState::Idle;
            self.clear_items(window, cx);
            cx.notify();
            return;
        };
        self.state = LoadState::Loading;
        self.clear_items(window, cx);
        cx.notify();

        let current = Arc::clone(&self.generation);
        let executor = cx.background_executor().clone();
        let task = cx.background_spawn(async move {
            executor.timer(delay).await;
            (current.load(Ordering::SeqCst) == generation).then(|| list_versions(kind, &game))
        });
        cx.spawn_in(window, async move |view, cx| {
            deliver_loader_listing(view, cx, generation, task.await).await;
        })
        .detach();
    }

    /// Whether a reply that is not from the newest request must be dropped.
    fn is_stale(&self, generation: u64) -> bool {
        is_stale(&self.generation, generation)
    }

    pub(super) fn finish(
        &mut self,
        generation: u64,
        result: Result<Vec<LoaderVersion>, LoaderError>,
        window: &mut Window,
        cx: &mut Context<'_, RootView>,
    ) {
        if self.is_stale(generation) {
            return;
        }
        match result {
            Ok(versions) if !versions.is_empty() => self.accept(versions, window, cx),
            Ok(_) => {
                let loader = self
                    .selection(cx)
                    .and_then(LoaderChoice::loader)
                    .map_or("This loader", kind_label);
                self.state = LoadState::Empty(format!(
                    "{loader} has no builds for Minecraft {}.",
                    self.asked_game
                ));
                self.versions.clear();
                self.clear_items(window, cx);
            }
            Err(error) => {
                self.state = LoadState::from_error(&error);
                self.versions.clear();
                self.clear_items(window, cx);
            }
        }
        cx.notify();
    }

    fn accept(
        &mut self,
        versions: Vec<LoaderVersion>,
        window: &mut Window,
        cx: &mut Context<'_, RootView>,
    ) {
        let items = SearchableVec::new(version_items(&versions));
        self.versions = versions;
        self.state = LoadState::Ready;
        self.version.update(cx, |state, cx| {
            state.set_items(items, window, cx);
            // The rows put the provider's recommendation first, so the
            // committed selection is the row a request without a version asks
            // for.
            state.set_selected_index(Some(IndexPath::new(0)), window, cx);
        });
    }

    fn clear_items(&mut self, window: &mut Window, cx: &mut Context<'_, RootView>) {
        let items = SearchableVec::new(Vec::<VersionItem>::new());
        self.version.update(cx, |state, cx| {
            state.set_items(items, window, cx);
            state.set_selected_index(None, window, cx);
        });
    }

    /// The loader and loader-version columns of the creation form.
    pub(super) fn render(&self, saving: bool, cx: &Context<'_, RootView>) -> AnyElement {
        let loader = self.selection(cx).and_then(LoaderChoice::loader);
        h_flex()
            .gap_3()
            .items_end()
            .child(
                v_flex()
                    .gap_1()
                    .w_40()
                    .child(Label::new("Loader").text_sm())
                    .child(
                        Select::new(&self.kind)
                            .id("new-instance-loader")
                            .accessibility_label("Loader")
                            .disabled(saving),
                    ),
            )
            .when_some(loader, |row, _| {
                row.child(
                    v_flex()
                        .gap_1()
                        .w_48()
                        .child(Label::new("Loader version").text_sm())
                        .child(
                            Select::new(&self.version)
                                .id("new-instance-loader-version")
                                .accessibility_label("Loader version")
                                .search_placeholder("Search versions")
                                .placeholder("Choose a version")
                                .disabled(saving || !matches!(self.state, LoadState::Ready)),
                        ),
                )
            })
            .into_any_element()
    }

    /// The loading, empty or failed sentence under the pickers, if any.
    pub(super) fn status(&self, cx: &Context<'_, RootView>) -> Option<AnyElement> {
        let kind = self.selection(cx)?.loader()?;
        let label = kind_label(kind);
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;
        let (text, color) = match &self.state {
            LoadState::Idle => (
                format!("Enter a Minecraft version to list its {label} builds."),
                muted,
            ),
            LoadState::Loading => (format!("Loading {label} versions…"), muted),
            LoadState::Ready => return None,
            LoadState::Empty(message) | LoadState::Failed(message) => (message.clone(), danger),
        };
        Some(
            Label::new(text)
                .text_xs()
                .text_color(color)
                .into_any_element(),
        )
    }
}

impl RootView {
    /// A newly confirmed loader kind lists its own provider's builds.
    ///
    /// The Minecraft version did not change, so this asks immediately rather
    /// than waiting for a text edit to settle.
    pub(super) fn loader_kind_changed(
        &mut self,
        _: &Entity<SelectState<SearchableVec<LoaderChoice>>>,
        event: &SelectEvent<SearchableVec<LoaderChoice>>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(event, SelectEvent::Confirm(_)) {
            self.refetch_loader(window, cx, Duration::ZERO);
        }
    }
}

/// Hands a fetch reply back to the view, unless a newer request replaced it.
///
/// A `None` reply means the request was overtaken before its delay elapsed,
/// and a view that has gone away has nothing left to show.
async fn deliver_loader_listing(
    view: WeakEntity<RootView>,
    cx: &mut AsyncWindowContext,
    generation: u64,
    result: Option<Result<Vec<LoaderVersion>, LoaderError>>,
) {
    let Some(result) = result else {
        return;
    };
    let _ = view.update_in(cx, |view, window, cx| {
        view.finish_loader_refetch(generation, result, window, cx);
    });
}

/// Whether a reply numbered `reply` belongs to the newest request.
///
/// Every request takes the next number, so only the reply carrying the
/// counter's current value describes what the form still asks for. Anything
/// else was overtaken while it was in flight and must not replace the list.
fn is_stale(generation: &AtomicU64, reply: u64) -> bool {
    reply != generation.load(Ordering::SeqCst)
}

/// One blocking round trip to a provider, on its own Tokio runtime.
///
/// Reqwest needs Tokio's I/O driver, and the caller runs this on a GPUI worker.
fn list_versions(kind: LoaderKind, game: &str) -> Result<Vec<LoaderVersion>, LoaderError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| LoaderError::Unavailable {
            loader: kind,
            source: FetchError::Fatal(error.to_string()),
        })?;
    runtime.block_on(async {
        let network = ReqwestNetwork::desktop().map_err(|source| LoaderError::Unavailable {
            loader: kind,
            source,
        })?;
        providers::versions(&network, kind, game).await
    })
}

#[cfg(test)]
#[path = "../../../../../tests/apps/frontend/src/panel/create_instance/loader.rs"]
mod tests;
