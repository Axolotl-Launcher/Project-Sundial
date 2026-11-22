// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::test_support::TempRoot;

fn job() -> Job {
    Job {
        id: "install-1".into(),
        kind: "install".into(),
        instance_id: Some("example".into()),
        status: JobStatus::Running,
        state: serde_json::json!({"stage": "libraries", "attempt": 2}),
        created: 1_700_000_000_000,
        modified: 1_700_000_001_000,
        finished: None,
        dismissed: false,
        items: vec![
            JobItem {
                id: "lib-1".into(),
                path: "libraries/a/a.jar".into(),
                state: JobItemState::Done,
                total_bytes: Some(10),
                written_bytes: 10,
                error: None,
            },
            JobItem {
                id: "lib-2".into(),
                path: "libraries/b/b.jar".into(),
                state: JobItemState::Pending,
                total_bytes: Some(20),
                written_bytes: 0,
                error: None,
            },
        ],
    }
}

#[test]
fn a_job_round_trips_with_its_opaque_state() {
    let root = TempRoot::new();
    let job = job();
    save(root.path(), &job).expect("save");

    let loaded = load(root.path(), &job.id).expect("load").expect("present");
    assert_eq!(loaded, job);
    assert_eq!(loaded.state["stage"], "libraries");
}

#[test]
fn outstanding_items_are_the_unfinished_ones() {
    let job = job();
    let pending: Vec<&str> = job.outstanding().map(|item| item.id.as_str()).collect();

    assert_eq!(pending, ["lib-2"]);
}

#[test]
fn a_missing_job_is_absent_rather_than_an_error() {
    let root = TempRoot::new();
    assert!(load(root.path(), "nothing").expect("load").is_none());
}

#[test]
fn an_id_that_could_escape_the_directory_is_refused() {
    let root = TempRoot::new();
    for id in ["", "../escape", "a/b", "a\\b", &"x".repeat(65)] {
        assert!(
            matches!(path(root.path(), id), Err(JobError::InvalidId { .. })),
            "{id:?} was accepted"
        );
    }
}

#[test]
fn another_schema_is_never_replaced() {
    let root = TempRoot::new();
    let path = path(root.path(), "install-1").expect("path");
    let document = br#"{"schema_version": 7, "job": {}}"#;
    fs::write(&path, document).expect("seed");

    let error = save(root.path(), &job()).expect_err("refused");
    assert!(matches!(error, JobError::RefusedSchema { found: 7, .. }));
    assert_eq!(fs::read(&path).expect("untouched"), document);
}
