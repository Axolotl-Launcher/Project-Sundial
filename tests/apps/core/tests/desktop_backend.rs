// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashSet;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use axlauncher_core::desktop::{
    DesktopBackend, DesktopOperationError, PendingDesktopBackend, operations,
};

#[test]
fn every_desktop_operation_has_a_distinct_key() {
    let listed: Vec<_> = operations::all().collect();
    let distinct: HashSet<_> = listed.iter().collect();

    assert!(listed.contains(&operations::instance::LIST));
    assert!(listed.contains(&operations::install::CREATE_INSTANCE));
    assert_eq!(distinct.len(), listed.len());
}

#[test]
fn the_pending_provider_identifies_each_unimplemented_operation() {
    let context = &mut Context::from_waker(Waker::noop());
    let backend = PendingDesktopBackend;

    for operation in operations::all() {
        let mut request = pin!(backend.request(operation));
        assert_eq!(
            request.as_mut().poll(context),
            Poll::Ready(Err(DesktopOperationError::NotImplemented { operation }))
        );
    }
}
