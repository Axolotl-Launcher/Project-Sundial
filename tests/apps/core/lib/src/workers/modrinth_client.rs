use axlauncher_storage::Directory;

use super::*;
use crate::Images;

#[test]
fn slots_share_only_a_matching_snapshot_and_never_start_during_registration() {
    let image = Images::new(b"", b"", b"", b"", b"", b"").modrinth;
    let workers = Workers::new(image, Err("unused executable".to_owned()));
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path().join("primary")).unwrap();
    let first = workers.for_data(&data).unwrap();
    assert!(Arc::ptr_eq(&first, &workers.for_data(&data).unwrap()));
    assert!(first.running_id().is_none());

    let other = DataDir::isolated(root.path().join("secondary")).unwrap();
    let mut different_cache = data.clone();
    different_cache
        .set_directory(Directory::AppCache, Some(&root.path().join("new-cache")))
        .unwrap();
    let mut different_profiles = data.clone();
    different_profiles
        .set_directory(Directory::UserData, Some(&root.path().join("new-profiles")))
        .unwrap();
    for scope in [other, different_cache, different_profiles] {
        let slot = workers.for_data(&scope).unwrap();
        assert!(!Arc::ptr_eq(&first, &slot));
        assert!(slot.running_id().is_none());
    }
    for worker in workers.take() {
        worker.close_and_wait();
    }
    assert!(matches!(
        workers.for_data(&data),
        Err(CatalogError::WorkerUnavailable)
    ));
}

#[tokio::test]
async fn client_accepts_one_typed_reply_only_after_the_terminal_frame() {
    let typed_error = Reply::new(None, Err(CatalogError::NotFound));
    let mut events = [Event::Reply(typed_error), Event::Finished(Ok(()))].into_iter();
    let reply = receive(async || events.next()).await.unwrap();
    assert!(matches!(
        reply.into_parts(),
        (None, Err(CatalogError::NotFound))
    ));

    let mut events = [
        Event::Reply(Reply::new(None, Ok(Payload::Versions(Vec::new())))),
        Event::Disconnected,
    ]
    .into_iter();
    let error = receive(async || events.next()).await.unwrap_err();
    assert_eq!(error, CatalogError::WorkerUnavailable);

    let error = receive(async || Some(Event::Finished(Ok(()))))
        .await
        .unwrap_err();
    assert_eq!(error, CatalogError::WorkerProtocol);
}

#[test]
fn catalog_cooldown_does_not_round_down_or_extend_an_expired_window() {
    let now = Instant::now();
    let until = now + Duration::from_millis(1001);
    assert_eq!(
        check_cooldown(Some(until), now),
        Err(CatalogError::RateLimited {
            retry_after_seconds: 2
        })
    );
    assert_eq!(check_cooldown(Some(until), until), Ok(()));
}

#[tokio::test]
async fn concurrent_catalog_scopes_preserve_a_rate_limit() {
    let cooldown = tokio::sync::Mutex::new(None);
    let (release, wait_for_release) = tokio::sync::oneshot::channel();
    let mut first_scope = std::pin::pin!(execute_with_cooldown(&cooldown, async {
        wait_for_release.await.unwrap();
        Ok(Reply::new(None, Ok(Payload::Versions(Vec::new()))))
    }));
    assert!(futures::poll!(first_scope.as_mut()).is_pending());

    let retry_after_seconds = 60;
    let mut second_scope = std::pin::pin!(execute_with_cooldown(&cooldown, async {
        Ok(Reply::new(
            Some(retry_after_seconds),
            Err(CatalogError::RateLimited {
                retry_after_seconds,
            }),
        ))
    }));
    let std::task::Poll::Ready(result) = futures::poll!(second_scope.as_mut()) else {
        panic!("the second scope must not wait for the first scope's reply");
    };
    assert_eq!(
        result.unwrap_err(),
        CatalogError::RateLimited {
            retry_after_seconds,
        }
    );
    let deadline = cooldown.lock().await.unwrap();

    release.send(()).unwrap();
    assert!(matches!(first_scope.await, Ok(Payload::Versions(_))));
    assert_eq!(*cooldown.lock().await, Some(deadline));

    let error = execute_with_cooldown(&cooldown, async {
        panic!("a request must not reach a worker during the cooldown");
    })
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        CatalogError::RateLimited {
            retry_after_seconds: 1..,
        }
    ));
}
