// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Live download diagnostics and a standalone transfer through the shared engine.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use axlauncher_core::desktop::downloads;
use axlauncher_core::desktop::sources::DownloadOptions;
use axlauncher_core::spi::DownloadProtocol;
use axlauncher_core::spi::downloads::{DownloadJob, FileDownloader, TransferStats};
use axlauncher_core::state::SettingsData;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, AsyncApp, BackgroundExecutor, Button, ButtonVariants, Context,
    Disableable, ElementId, Entity, GroupBox, Input, InputEvent, InputState, IntoElement, Label,
    Task, WeakEntity, Window, h_flex, v_flex,
};
use futures::future::{Either, select};
use tokio::sync::oneshot;

use super::RootView;
use crate::installations::Installation;
use crate::modpacks::ModpackImport;

mod batch;
mod sources;

// Cargo Shear scans imports inside the crate but misses the mirrored tests.
#[cfg(test)]
use tempfile as test_tempfile;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(300);
const MIB: f64 = (1024 * 1024) as f64;
const MAX_VISIBLE_INSTALLATIONS: usize = 8;

pub(super) struct DownloadDebug {
    url: Entity<InputState>,
    batch: batch::BatchDebug,
    sources: sources::DownloadSources,
    stats: TransferStats,
    sampled_at: Instant,
    bytes_per_second: f64,
    status: ProbeStatus,
    cancel: Option<oneshot::Sender<()>>,
    sampler: Option<Task<()>>,
}

enum ProbeStatus {
    Idle,
    Running,
    Cancelling,
    Cancelled,
    Complete(PathBuf),
    Failed(String),
}

impl ProbeStatus {
    fn is_running(&self) -> bool {
        matches!(self, Self::Running | Self::Cancelling)
    }

    fn message(&self) -> Option<String> {
        match self {
            Self::Idle => None,
            Self::Running => Some("Test download running…".into()),
            Self::Cancelling => Some("Cancelling test download…".into()),
            Self::Cancelled => Some("Test download cancelled".into()),
            Self::Complete(path) => Some(format!("Saved {}", path.display())),
            Self::Failed(error) => Some(error.clone()),
        }
    }
}

impl DownloadDebug {
    pub(super) fn new(
        settings: Option<&SettingsData>,
        window: &mut Window,
        cx: &mut Context<'_, RootView>,
    ) -> Self {
        let url = cx.new(|cx| InputState::new(window, cx).placeholder("https://…"));
        cx.subscribe(&url, RootView::download_probe_event).detach();
        Self {
            url,
            batch: batch::BatchDebug::new(window, cx),
            sources: sources::DownloadSources::new(settings, window, cx),
            stats: downloads::stats(),
            sampled_at: Instant::now(),
            bytes_per_second: 0.,
            status: ProbeStatus::Idle,
            cancel: None,
            sampler: None,
        }
    }

    pub(super) fn reload_sources(
        &mut self,
        settings: Option<&SettingsData>,
        window: &mut Window,
        cx: &mut Context<'_, RootView>,
    ) {
        self.sources.sync(settings, window, cx);
        self.sources.error = None;
    }

    fn sample(&mut self) {
        let now = Instant::now();
        let new = downloads::stats();
        let delta = new.bytes_received.saturating_sub(self.stats.bytes_received);
        let elapsed = (now - self.sampled_at).as_secs_f64();
        if elapsed > 0. {
            self.bytes_per_second = delta as f64 / elapsed;
        }
        self.sampled_at = now;
        self.stats = new;
    }
}

impl RootView {
    pub(super) fn modpack_download_changed(
        &mut self,
        modpack: Entity<ModpackImport>,
        cx: &mut Context<'_, Self>,
    ) {
        if modpack.read(cx).install_progress().is_some() {
            self.monitor_downloads(cx);
        }
        cx.notify();
    }

    fn download_probe_event(
        &mut self,
        _: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(
            event,
            InputEvent::PressEnter {
                secondary: false,
                shift: false
            }
        ) {
            self.start_download_probe(cx);
        }
    }

    pub(super) fn monitor_downloads(&mut self, cx: &mut Context<'_, Self>) {
        if self.download_debug.sampler.is_some() {
            return;
        }
        self.download_debug.sample();
        let executor = cx.background_executor().clone();
        self.download_debug.sampler =
            Some(cx.spawn(async move |view, cx| sample_until_idle(executor, view, cx).await));
    }

    fn sample_downloads(&mut self, cx: &mut Context<'_, Self>) -> bool {
        self.download_debug.sample();
        self.download_debug.batch.drain_outcomes();
        let keep_sampling = self.download_debug.status.is_running()
            || self.download_debug.batch.is_running()
            || self.modpack.read(cx).install_progress().is_some()
            || self
                .installations
                .values()
                .flat_map(|versions| versions.values())
                .any(|state| matches!(state, Installation::Running(_)));
        if !keep_sampling {
            self.download_debug.sampler = None;
            self.download_debug.bytes_per_second = 0.;
        }
        cx.notify();
        keep_sampling
    }

    fn start_download_probe(&mut self, cx: &mut Context<'_, Self>) {
        if self.download_debug.status.is_running() {
            return;
        }
        let Some(paths) = self.paths.as_ref() else {
            return;
        };
        let destination = paths.download_debug_file();
        let url = self.download_debug.url.read(cx).value().trim().to_owned();
        if url.is_empty() {
            self.download_debug.status = ProbeStatus::Failed("Enter a download URL.".into());
            cx.notify();
            return;
        }
        let (cancel, cancellation) = oneshot::channel();
        self.download_debug.status = ProbeStatus::Running;
        self.download_debug.cancel = Some(cancel);
        let options = DownloadOptions::from_settings(self.settings.as_ref());
        let downloader = options.downloader(paths);
        self.monitor_downloads(cx);
        let task = cx
            .background_spawn(async move { run_probe(url, destination, downloader, cancellation) });
        cx.spawn(async move |view, cx| {
            let status = task.await;
            let _ = view.update(cx, |view, cx| view.finish_download_probe(status, cx));
        })
        .detach();
        cx.notify();
    }

    fn finish_download_probe(&mut self, status: ProbeStatus, cx: &mut Context<'_, Self>) {
        self.download_debug.cancel = None;
        self.download_debug.status = status;
        self.download_debug.sample();
        cx.notify();
    }

    fn cancel_download_probe(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(cancel) = self.download_debug.cancel.take() {
            let _ = cancel.send(());
            self.download_debug.status = ProbeStatus::Cancelling;
            cx.notify();
        }
    }

    pub(super) fn render_download_debug(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let state = &self.download_debug;
        let stats = state.stats;
        let status_color = match state.status {
            ProbeStatus::Failed(_) => cx.theme().danger,
            ProbeStatus::Complete(_) => cx.theme().success,
            _ => muted,
        };
        GroupBox::new()
            .id("download-debug")
            .title(Label::new("Download debug").text_sm().text_color(muted))
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_4()
                            .child(Label::new(protocol_label(stats.protocol)).text_sm())
                            .child(
                                Label::new(format!(
                                    "{} / {} active streams",
                                    stats.active_connections, stats.connection_limit
                                ))
                                .text_sm(),
                            )
                            .child(
                                Label::new(format!(
                                    "{} streams per file",
                                    stats.connections_per_file
                                ))
                                .text_sm(),
                            )
                            .child(
                                Label::new(format!("{:.2} MiB/s", state.bytes_per_second / MIB))
                                    .text_sm(),
                            )
                            .child(
                                Label::new(format!(
                                    "{:.2} MiB received",
                                    stats.bytes_received as f64 / MIB
                                ))
                                .text_sm(),
                            ),
                    )
                    .child(self.render_download_sources(cx))
                    .child(self.render_download_probe(cx))
                    .when_some(state.status.message(), |column, message| {
                        column.child(Label::new(message).text_sm().text_color(status_color))
                    })
                    .child(self.render_batch_probe(cx))
                    .children(self.render_modpack_progress(cx))
                    .children(self.render_installation_progress(cx)),
            )
            .into_any_element()
    }

    fn render_modpack_progress(&self, cx: &Context<'_, Self>) -> Option<Label> {
        let (name, progress) = self.modpack.read(cx).install_progress()?;
        Some(
            Label::new(format!("{name} · {}", progress.message()))
                .id(ElementId::Name("download-progress:modpack".into()))
                .text_sm()
                .text_color(cx.theme().muted_foreground),
        )
    }

    fn render_download_probe(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let state = &self.download_debug;
        let running = state.status.is_running();
        v_flex()
            .gap_1()
            .child(Label::new("Test URL").text_sm())
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Input::new(&state.url)
                            .id("download-debug-url")
                            .aria_label("Test URL")
                            .min_w_0()
                            .flex_1()
                            .disabled(running),
                    )
                    .child(
                        Button::new("start-download-probe")
                            .label("Test download")
                            .disabled(running || self.paths.is_none())
                            .on_click(cx.listener(|view, _, _, cx| view.start_download_probe(cx))),
                    )
                    .when(running, |row| {
                        row.child(
                            Button::new("cancel-download-probe")
                                .label("Cancel")
                                .ghost()
                                .disabled(state.cancel.is_none())
                                .on_click(
                                    cx.listener(|view, _, _, cx| view.cancel_download_probe(cx)),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_installation_progress(&self, cx: &Context<'_, Self>) -> Vec<AnyElement> {
        let Some(root) = self.paths.as_ref().map(|paths| paths.game()) else {
            return Vec::new();
        };
        let Some(versions) = self.installations.get(root.root()) else {
            return Vec::new();
        };
        let mut active: Vec<_> = versions
            .iter()
            .filter(|(_, state)| matches!(state, Installation::Running(_)))
            .collect();
        active.sort_unstable_by_key(|(version, _)| version.as_str());
        active
            .into_iter()
            .take(MAX_VISIBLE_INSTALLATIONS)
            .map(|(version, state)| {
                Label::new(format!("{version} · {}", state.message()))
                    .id(ElementId::Name(
                        format!("download-progress:{version}").into(),
                    ))
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .into_any_element()
            })
            .collect()
    }
}

async fn sample_until_idle(
    executor: BackgroundExecutor,
    view: WeakEntity<RootView>,
    cx: &mut AsyncApp,
) {
    loop {
        executor.timer(SAMPLE_INTERVAL).await;
        if !matches!(view.update(cx, RootView::sample_downloads), Ok(true)) {
            break;
        }
    }
}

fn protocol_label(protocol: DownloadProtocol) -> &'static str {
    match protocol {
        DownloadProtocol::Http2 => "HTTP/2",
        DownloadProtocol::Http1 => "HTTP/1.1",
        DownloadProtocol::Automatic => "Automatic HTTP",
    }
}

fn run_probe(
    url: String,
    destination: PathBuf,
    downloader: impl FileDownloader,
    cancel: oneshot::Receiver<()>,
) -> ProbeStatus {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            return ProbeStatus::Failed(format!("Cannot start the download runtime: {error}"));
        }
    };
    runtime.block_on(async {
        let job = DownloadJob {
            sources: &[&url],
            destination: &destination,
            sha1: None,
            sha1_alternatives: &[],
            size: None,
            batch: false,
        };
        let transfer = std::pin::pin!(downloader.download(job, None));
        match select(cancel, transfer).await {
            Either::Left(_) => ProbeStatus::Cancelled,
            Either::Right((Ok(()), _)) => ProbeStatus::Complete(destination.clone()),
            Either::Right((Err(error), _)) => ProbeStatus::Failed(error),
        }
    })
}

#[cfg(test)]
#[path = "../../../../tests/apps/frontend/src/panel/download_debug.rs"]
mod tests;
