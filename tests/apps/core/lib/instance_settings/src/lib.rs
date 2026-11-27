use super::*;

#[test]
fn heap_edits_keep_the_modes_distinct_at_the_minimum() {
    assert_eq!(
        validate(Edit::GlobalHeap(GlobalHeap::Automatic)),
        Ok(ValidatedEdit::GlobalHeap(MemoryAllocation::Automatic))
    );
    assert_eq!(
        validate(Edit::InstanceHeap(InstanceHeap::Inherit)),
        Ok(ValidatedEdit::InstanceHeap(None))
    );
    let heap = MaximumHeap::new(16).unwrap();
    assert_eq!(
        validate(Edit::GlobalHeap(GlobalHeap::ManualMib(16))),
        Ok(ValidatedEdit::GlobalHeap(MemoryAllocation::Manual(heap)))
    );
    assert_eq!(
        validate(Edit::InstanceHeap(InstanceHeap::ManualMib(16))),
        Ok(ValidatedEdit::InstanceHeap(Some(heap)))
    );
    for invalid in [
        Edit::GlobalHeap(GlobalHeap::ManualMib(15)),
        Edit::InstanceHeap(InstanceHeap::ManualMib(15)),
    ] {
        assert_eq!(validate(invalid), Err(InvalidEdit::InvalidHeap));
    }
}

#[test]
fn a_name_is_trimmed_but_cannot_be_blank_or_contain_controls() {
    assert_eq!(
        validate(Edit::Rename("  Launch / 雪  ".into())),
        Ok(ValidatedEdit::Rename("Launch / 雪".into()))
    );
    for invalid in ["   ", "Launch\nName", "null\0"] {
        assert_eq!(
            validate(Edit::Rename(invalid.into())),
            Err(InvalidEdit::InvalidName)
        );
    }
}
