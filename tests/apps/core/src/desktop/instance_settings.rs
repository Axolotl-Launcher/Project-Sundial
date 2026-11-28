use super::*;

#[test]
fn a_successful_reply_must_validate_the_exact_requested_edit() {
    let request = Edit::InstanceHeap(InstanceHeap::ManualMib(16));
    let heap = MaximumHeap::new(16).unwrap();
    let accepted = || Reply::Validated(ValidatedEdit::InstanceHeap(Some(heap)));
    assert_eq!(
        verify_reply(&request, accepted()).unwrap(),
        ValidatedEdit::InstanceHeap(Some(heap))
    );
    assert!(verify_reply(&Edit::InstanceHeap(InstanceHeap::ManualMib(17)), accepted()).is_err());
    assert!(verify_reply(&Edit::GlobalHeap(GlobalHeap::ManualMib(16)), accepted()).is_err());
    assert!(verify_reply(&Edit::InstanceHeap(InstanceHeap::Inherit), accepted()).is_err());
    let request = Edit::Rename("  Edition  ".into());
    assert!(
        verify_reply(
            &request,
            Reply::Validated(ValidatedEdit::Rename(" Edition".into()))
        )
        .is_err()
    );
    assert!(
        verify_reply(
            &request,
            Reply::Validated(ValidatedEdit::Rename("Other".into()))
        )
        .is_err()
    );
    assert!(
        verify_reply(
            &request,
            Reply::Validated(ValidatedEdit::Rename("Edition".into()))
        )
        .is_ok()
    );
}

#[test]
fn unconfigured_worker_has_no_write_fallback() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let document = axlauncher_storage::Document::Settings.path(root.path());
    assert!(matches!(
        set_global_heap(
            &data,
            MemoryAllocation::Automatic,
            GlobalHeap::ManualMib(2048),
            &EditCancellation::default()
        ),
        Err(EditError::Worker(_))
    ));
    assert!(!document.exists());
}

#[test]
fn a_stale_scope_cannot_edit_an_instance_in_the_same_shared_profile_root() {
    let root = tempfile::tempdir().unwrap();
    let first = DataDir::isolated(root.path().join("first")).unwrap();
    let mut other = DataDir::isolated(root.path().join("other")).unwrap();
    let profiles = first.path(DataPath::Profiles);
    other
        .set_directory(axlauncher_storage::Directory::UserData, Some(profiles))
        .unwrap();
    profile::create(profiles, "First", profile::Game::new("1.21.11")).unwrap();
    let discovered = profile::discover(profiles);
    let instance = InstanceTarget::new(&discovered.instances[0], &first);
    assert!(matches!(
        rename(
            &other,
            &instance,
            "Wrong scope",
            &EditCancellation::default()
        ),
        Err(EditError::ChangedScope)
    ));
    assert_eq!(
        profile::read(instance.directory())
            .unwrap()
            .unwrap()
            .instance
            .name,
        "First"
    );
}
