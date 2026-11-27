use super::*;
use crate::instance_settings::InvalidEdit;

fn rejection() -> Reply {
    Reply::Rejected(InvalidEdit::InvalidHeap)
}

#[tokio::test]
async fn one_reply_requires_a_successful_terminal_frame() {
    let mut good = [Event::Reply(rejection()), Event::Finished(Ok(()))].into_iter();
    assert_eq!(receive(async || good.next()).await, Ok(rejection()));
    let mut failed = [
        Event::Reply(rejection()),
        Event::Finished(Err("exited".into())),
    ]
    .into_iter();
    assert_eq!(receive(async || failed.next()).await, Err("exited".into()));
}

#[tokio::test]
async fn a_duplicate_missing_or_truncated_reply_fails() {
    for frames in [
        vec![
            Event::Reply(rejection()),
            Event::Reply(rejection()),
            Event::Finished(Ok(())),
        ],
        vec![Event::Finished(Ok(()))],
        vec![Event::Reply(rejection())],
    ] {
        let mut frames = frames.into_iter();
        assert!(receive(async || frames.next()).await.is_err());
    }
}
