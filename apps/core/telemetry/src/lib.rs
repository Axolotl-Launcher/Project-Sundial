// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Observability: event sink and spans.

use superlogical_spi::{Event, Telemetry};

/// Writes events to a log target; a real exporter replaces it.
#[derive(Debug, Default)]
pub struct LoggingTelemetry;

impl Telemetry for LoggingTelemetry {
    fn record(&self, event: Event) {
        eprintln!("[telemetry] {}", event.name);
    }
}
