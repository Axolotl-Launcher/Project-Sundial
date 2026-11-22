// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Keeps every port error usable as an error.
//!
//! A port error that lacks `Display` cannot be formatted into a caller's own
//! message, and one that lacks `Error` cannot be a `source`. Both gaps surfaced
//! one at a time while wiring systems together, each discovered only when a
//! caller tried to use it. This test asks the question once, at compile time,
//! so the next port error is written correctly from the start.

use std::error::Error;
use std::fmt::Display;

fn is_display<E: Display>() {}
fn is_error<E: Error>() {}
fn is_display_and_error<E: Display + Error>() {}

#[test]
fn every_port_error_is_display_and_error() {
    is_display::<axlauncher_spi::FetchError>();
    is_error::<axlauncher_spi::FetchError>();

    is_display::<axlauncher_spi::StorageError>();
    is_error::<axlauncher_spi::StorageError>();

    is_display::<axlauncher_spi::InstallError>();
    is_error::<axlauncher_spi::InstallError>();

    is_display::<axlauncher_spi::LaunchError>();
    is_error::<axlauncher_spi::LaunchError>();

    is_display_and_error::<axlauncher_spi::FetchError>();
    is_display_and_error::<axlauncher_spi::StorageError>();
    is_display_and_error::<axlauncher_spi::InstallError>();
    is_display_and_error::<axlauncher_spi::LaunchError>();
}

#[test]
fn a_port_error_reads_as_a_sentence() {
    let fatal = axlauncher_spi::FetchError::Fatal("no route to host".into());
    assert_eq!(fatal.to_string(), "no route to host");

    let retryable = axlauncher_spi::FetchError::Retryable("timed out".into());
    assert!(retryable.to_string().contains("timed out"));

    let exited = axlauncher_spi::LaunchError::Exited { code: Some(3) };
    assert!(exited.to_string().contains('3'));

    let killed = axlauncher_spi::LaunchError::Exited { code: None };
    assert!(killed.to_string().contains("terminated"));

    let missing = axlauncher_spi::LaunchError::NotFound("no such file".into());
    assert!(missing.to_string().contains("no such file"));
}
