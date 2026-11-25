// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn one_throttle_preserves_phases_small_downloads_and_the_final_count() {
    let (progress, mut updates) = watch::channel(Progress::Version);
    let total = 301;
    report_progress(&progress, Progress::Loader);
    assert_eq!(*updates.borrow_and_update(), Progress::Loader);
    report_progress(
        &progress,
        Progress::Downloading {
            processed: 1,
            total,
        },
    );
    assert!(!updates.has_changed().unwrap());
    report_progress(
        &progress,
        Progress::Downloading {
            processed: 3,
            total,
        },
    );
    assert_eq!(
        *updates.borrow_and_update(),
        Progress::Downloading {
            processed: 3,
            total
        }
    );
    report_progress(
        &progress,
        Progress::Downloading {
            processed: total,
            total,
        },
    );
    assert_eq!(
        *updates.borrow_and_update(),
        Progress::Downloading {
            processed: total,
            total
        }
    );
    report_progress(
        &progress,
        Progress::Downloading {
            processed: 1,
            total: 7,
        },
    );
    assert_eq!(
        *updates.borrow_and_update(),
        Progress::Downloading {
            processed: 1,
            total: 7
        }
    );
}
