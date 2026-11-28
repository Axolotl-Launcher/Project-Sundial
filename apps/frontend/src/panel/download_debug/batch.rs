// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The development panel's multiline batch transfer and result list.

use std::ops::Range;
use std::path::{Path, PathBuf};

use axlauncher_core::download_spi::{DownloadOutcome, FileDownloader, MissingFile};
use axlauncher_core::storage::{DataDir, ManagedFile};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, Button, ButtonVariants, Context, Disableable, ElementId, Entity,
    Focusable, IntoElement, Label, Pagination, Render, Sizable, Textarea, TextareaState, Window,
    div, h_flex, v_flex,
};
use futures::{FutureExt, StreamExt};
use tokio::sync::{mpsc, oneshot};

use crate::i18n::tr;

use super::DownloadOptions;

const RESULTS_PER_PAGE: usize = 10;

pub(super) struct BatchDebug {
    paths: Option<DataDir>,
    options: DownloadOptions,
    input: Entity<TextareaState>,
    expanded: bool,
    rows: Vec<BatchRow>,
    result_page: usize,
    counts: Counts,
    phase: Phase,
    cancel: Option<oneshot::Sender<()>>,
    outcomes: Option<mpsc::UnboundedReceiver<DownloadOutcome>>,
}

struct BatchRow {
    line: usize,
    result: Option<Result<(), String>>,
}

#[derive(Default)]
struct Counts {
    saved: usize,
    failed: usize,
}

enum Phase {
    Idle,
    Running,
    Cancelling,
    Finished(BatchReport),
    Failed(String),
}

struct BatchReport {
    directory: PathBuf,
    cancelled: bool,
}

struct Link {
    line: usize,
    url: String,
}

impl BatchDebug {
    pub(super) fn new(
        paths: Option<DataDir>,
        options: DownloadOptions,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        Self {
            paths,
            options,
            input: cx.new(|cx| {
                TextareaState::new(window, cx).placeholder("One HTTP or HTTPS URL per line")
            }),
            expanded: false,
            rows: Vec::new(),
            result_page: 1,
            counts: Counts::default(),
            phase: Phase::Idle,
            cancel: None,
            outcomes: None,
        }
    }

    pub(super) fn configure(
        &mut self,
        paths: Option<DataDir>,
        options: DownloadOptions,
        cx: &mut Context<'_, Self>,
    ) {
        let enabled_changed = self.paths.is_some() != paths.is_some();
        self.paths = paths;
        self.options = options;
        if enabled_changed {
            cx.notify();
        }
    }

    pub(super) fn is_running(&self) -> bool {
        matches!(self.phase, Phase::Running | Phase::Cancelling)
    }

    pub(super) fn sample(&mut self, cx: &mut Context<'_, Self>) {
        let previous = self.counts.saved + self.counts.failed;
        self.drain_outcomes();
        if previous != self.counts.saved + self.counts.failed {
            cx.notify();
        }
    }

    fn drain_outcomes(&mut self) {
        let Some(receiver) = &mut self.outcomes else {
            return;
        };
        while let Ok(outcome) = receiver.try_recv() {
            let line = output_line(&outcome.path);
            let index = self
                .rows
                .binary_search_by_key(&line, |row| row.line)
                .expect("batch output matches an input line");
            self.counts.saved += usize::from(outcome.result.is_ok());
            self.counts.failed += usize::from(outcome.result.is_err());
            self.rows[index].result = Some(outcome.result);
        }
    }

    fn summary(&self) -> Option<String> {
        let state = match self.phase {
            Phase::Idle => return None,
            Phase::Failed(ref error) => return Some(error.clone()),
            Phase::Running => "Downloading",
            Phase::Cancelling => "Cancelling",
            Phase::Finished(ref report) if report.cancelled => "Cancelled",
            Phase::Finished(_) => "Finished",
        };
        let Counts { saved, failed } = self.counts;
        Some(format!(
            "{state} · {} / {} processed · {saved} saved · {failed} failed",
            saved + failed,
            self.rows.len()
        ))
    }
    fn toggle_batch_probe(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.expanded = !self.expanded;
        if self.expanded {
            self.input.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn start_batch_probe(&mut self, cx: &mut Context<'_, Self>) {
        if self.is_running() {
            return;
        }
        let Some(paths) = self.paths.clone() else {
            return;
        };
        let links = parse_links(&self.input.read(cx).value());
        self.result_page = 1;
        self.rows = links
            .iter()
            .map(|link| BatchRow {
                line: link.line,
                result: None,
            })
            .collect();
        self.counts = Counts::default();
        if links.is_empty() {
            self.phase = Phase::Failed("Enter at least one download URL.".into());
            cx.notify();
            return;
        }
        let downloader = self.options.downloader(&paths);
        let concurrency = self.options.concurrency();
        let (cancel, cancellation) = oneshot::channel();
        let (report, outcomes) = mpsc::unbounded_channel();
        self.phase = Phase::Running;
        self.cancel = Some(cancel);
        self.outcomes = Some(outcomes);
        let task = cx.background_spawn(async move {
            run_batch(
                &paths,
                links,
                &downloader,
                concurrency,
                cancellation,
                report,
            )
        });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| view.finish_batch_probe(result, cx));
        })
        .detach();
        cx.notify();
    }

    fn finish_batch_probe(
        &mut self,
        result: Result<BatchReport, String>,
        cx: &mut Context<'_, Self>,
    ) {
        self.drain_outcomes();
        self.outcomes = None;
        self.cancel = None;
        self.phase = result.map_or_else(Phase::Failed, Phase::Finished);
        cx.notify();
    }

    fn cancel_batch_probe(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
            self.phase = Phase::Cancelling;
            cx.notify();
        }
    }

    fn render_batch_form(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let running = self.is_running();
        let color = if matches!(self.phase, Phase::Failed(_)) {
            cx.theme().danger
        } else {
            cx.theme().muted_foreground
        };
        v_flex()
            .gap_2()
            .child(Label::new(tr(cx, "download-debug.batch.label")).text_sm())
            .child(
                Textarea::new(&self.input)
                    .accessibility_id("download-batch-urls")
                    .aria_label(tr(cx, "download-debug.batch.hint"))
                    .w_full()
                    .h_24()
                    .readonly(running),
            )
            .child(self.render_batch_actions(cx))
            .when_some(self.summary(), |panel, summary| {
                panel.child(Label::new(summary).text_sm().text_color(color))
            })
            .when_some(self.output_directory(), |panel, directory| {
                panel.child(
                    div()
                        .text_xs()
                        .text_color(color)
                        .child(directory.display().to_string()),
                )
            })
            .when(!self.rows.is_empty(), |panel| {
                panel.child(self.render_batch_results(cx))
            })
            .into_any_element()
    }

    fn render_batch_actions(&self, cx: &mut Context<'_, Self>) -> impl IntoElement + use<> {
        h_flex()
            .gap_2()
            .child(
                Button::new("start-batch-download")
                    .label(tr(cx, "download-debug.batch.start"))
                    .disabled(self.is_running() || self.paths.is_none())
                    .on_click(cx.listener(|view, _, _, cx| view.start_batch_probe(cx))),
            )
            .when(self.is_running(), |row| {
                row.child(
                    Button::new("cancel-batch-download")
                        .label(tr(cx, "download-debug.batch.cancel"))
                        .ghost()
                        .disabled(self.cancel.is_none())
                        .on_click(cx.listener(|view, _, _, cx| view.cancel_batch_probe(cx))),
                )
            })
    }

    fn render_batch_results(&self, cx: &mut Context<'_, Self>) -> impl IntoElement + use<> {
        let total = self.rows.len();
        let pages = total.div_ceil(RESULTS_PER_PAGE);
        let range = result_page_range(total, self.result_page);
        let muted = cx.theme().muted_foreground;
        let rows = self.rows[range.clone()]
            .iter()
            .map(|row| row.render(&self.phase, cx))
            .collect::<Vec<_>>();
        v_flex()
            .w_full()
            .gap_1()
            .when(pages > 1, |column| {
                column.child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .justify_between()
                        .child(
                            Label::new(format!(
                                "Results {}–{} of {total}",
                                range.start + 1,
                                range.end
                            ))
                            .text_sm()
                            .text_color(muted),
                        )
                        .child(
                            Pagination::new("batch-result-pages")
                                .small()
                                .current_page(self.result_page)
                                .total_pages(pages)
                                .on_click(cx.listener(Self::select_batch_result_page)),
                        ),
                )
            })
            .children(rows)
    }

    fn select_batch_result_page(
        &mut self,
        page: &usize,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.result_page = *page;
        cx.notify();
    }
}

impl Render for BatchDebug {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let toggle = Button::new("show-batch-download")
            .label(if self.expanded {
                "Hide batch download"
            } else {
                "Batch download…"
            })
            .on_click(cx.listener(|view, _, window, cx| view.toggle_batch_probe(window, cx)));
        v_flex()
            .gap_2()
            .child(h_flex().child(toggle))
            .when(self.expanded, |column| {
                column.child(self.render_batch_form(cx))
            })
    }
}

fn result_page_range(total: usize, page: usize) -> Range<usize> {
    let last_page = total.div_ceil(RESULTS_PER_PAGE).max(1);
    let start = (page.clamp(1, last_page) - 1) * RESULTS_PER_PAGE;
    start..start.saturating_add(RESULTS_PER_PAGE).min(total)
}

impl BatchDebug {
    fn output_directory(&self) -> Option<&Path> {
        match &self.phase {
            Phase::Finished(report) => Some(&report.directory),
            _ => None,
        }
    }
}

impl BatchRow {
    fn render(&self, phase: &Phase, cx: &App) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let (message, color) = match self.result.as_ref() {
            Some(Ok(())) => (format!("Saved {}.bin", self.line), cx.theme().success),
            Some(Err(error)) => (format!("Failed: {error}"), cx.theme().danger),
            None if matches!(phase, Phase::Finished(report) if report.cancelled) => {
                ("Cancelled".into(), muted)
            }
            None if matches!(phase, Phase::Failed(_)) => ("Not started".into(), muted),
            None => ("Pending".into(), muted),
        };
        h_flex()
            .gap_3()
            .h_7()
            .px_2()
            .id(ElementId::named_usize("batch-result", self.line))
            .child(
                div()
                    .flex_shrink_0()
                    .w_16()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("Line {}", self.line)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(color)
                    .child(message),
            )
            .into_any_element()
    }
}

fn parse_links(input: &str) -> Vec<Link> {
    input
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let url = line.trim();
            (!url.is_empty()).then(|| Link {
                line: index + 1,
                url: url.to_owned(),
            })
        })
        .collect()
}

fn output_line(path: &Path) -> usize {
    path.file_stem()
        .and_then(|name| name.to_str())
        .and_then(|name| name.parse().ok())
        .expect("numeric batch output name")
}

fn run_batch(
    data: &DataDir,
    links: Vec<Link>,
    downloader: &impl FileDownloader,
    concurrency: usize,
    cancellation: oneshot::Receiver<()>,
    report: mpsc::UnboundedSender<DownloadOutcome>,
) -> Result<BatchReport, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let directory = data
        .new_download_debug_batch()
        .map_err(|error| format!("Cannot create the batch output directory: {error}"))?;
    let files: Vec<_> = links
        .into_iter()
        .map(|link| MissingFile {
            url: link.url,
            path: ManagedFile::DownloadBatchOutput(link.line).path(&directory),
            sha1: None,
            sha1_alternatives: Vec::new(),
            size: None,
        })
        .collect();
    let cancelled = runtime.block_on(transfer_batch(
        &files,
        downloader,
        concurrency,
        cancellation,
        report,
    ))?;
    Ok(BatchReport {
        directory,
        cancelled,
    })
}

async fn transfer_batch(
    files: &[MissingFile],
    downloader: &impl FileDownloader,
    concurrency: usize,
    cancellation: oneshot::Receiver<()>,
    report: mpsc::UnboundedSender<DownloadOutcome>,
) -> Result<bool, String> {
    let downloads = downloader
        .batch(files, concurrency)
        .take_until(cancellation.map(|_| ()));
    let mut downloads = std::pin::pin!(downloads);
    let mut processed = 0;
    while let Some(outcome) = downloads.next().await {
        if report.send(outcome).is_err() {
            return Ok(true);
        }
        processed += 1;
    }
    Ok(processed < files.len())
}

#[cfg(test)]
#[path = "../../../../../tests/apps/frontend/src/panel/download_debug/batch.rs"]
mod tests;
