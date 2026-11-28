use super::*;

#[test]
fn reloading_the_scope_cancels_an_uncommitted_edit() {
    let mut edits = MemoryEdits::default();
    let original = edits.scope();
    let request = Arc::new(EditCancellation::default());
    edits.begin(&request);
    edits.invalidate();
    assert!(!request.begin_commit());
    assert!(!edits.scope().matches(&original));
}

#[test]
fn dropping_the_view_cancels_only_before_commit() {
    for committing in [false, true] {
        let request = Arc::new(EditCancellation::default());
        let mut edits = MemoryEdits::default();
        edits.begin(&request);
        if committing {
            assert!(request.begin_commit());
        }
        drop(edits);
        assert!(!request.is_pending());
        assert_eq!(request.is_saving(), committing);
    }
}

#[test]
fn a_completion_must_belong_to_the_current_scope_and_request() {
    let mut edits = MemoryEdits::default();
    let scope = edits.scope();
    let request = Arc::new(EditCancellation::default());
    let active = edits.begin(&request);
    assert!(edits.accepts(&scope, &active));
    assert!(!edits.accepts(&MemoryScope::default(), &active));
    let other = Arc::new(EditCancellation::default());
    assert!(!edits.accepts(&scope, &Arc::downgrade(&other)));
    edits.invalidate();
    assert!(!edits.accepts(&scope, &active));
}
