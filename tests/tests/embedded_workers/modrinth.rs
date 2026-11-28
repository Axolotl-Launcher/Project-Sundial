use axlauncher_worker::modrinth::{CatalogError, Command, Reply};
use axlauncher_worker::{Event, Request, Service};
use serde_json::json;

use super::{Fixture, finish, kill, run};

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn catalog_image_starts_lazily_reuses_its_lease_and_recovers_after_a_crash() {
    let fixture = Fixture::new();
    let worker =
        Service::<Reply>::for_executable(fixture.images.modrinth, fixture.executable.clone());
    assert!(worker.running_id().is_none());
    assert!(!fixture.has_extracted_libraries());
    let lease = worker.hold();
    assert!(!fixture.has_extracted_libraries());

    let mut first = rejected_lookup(&fixture, &worker);
    let pid = worker.running_id().unwrap();
    assert!(fixture.has_extracted_libraries());
    run(assert_rejected(&mut first));
    run(assert_rejected(&mut rejected_lookup(&fixture, &worker)));
    assert_eq!(worker.running_id(), Some(pid));

    kill(pid);
    fixture.wait_for_cleanup();
    run(assert_rejected(&mut rejected_lookup(&fixture, &worker)));
    assert_ne!(worker.running_id().unwrap(), pid);
    drop(lease);
    fixture.wait_for_cleanup();
    assert!(worker.running_id().is_none());

    let _lease = worker.hold();
    run(assert_rejected(&mut rejected_lookup(&fixture, &worker)));
    worker.close_and_wait();
    fixture.wait_for_cleanup();
    assert!(worker.running_id().is_none());
}

fn rejected_lookup(fixture: &Fixture, worker: &Service<Reply>) -> Request<Reply> {
    let command: Command = serde_json::from_value(json!({"V1": {"Project": ".."}})).unwrap();
    worker.request(&fixture.data, command).unwrap()
}

async fn assert_rejected(request: &mut Request<Reply>) {
    let Some(Event::Reply(reply)) = request.next().await else {
        panic!("expected a typed catalog reply");
    };
    let (cooldown, result) = reply.into_parts();
    assert!(cooldown.is_none());
    assert!(matches!(result, Err(CatalogError::InvalidRequest)));
    finish(request).await.unwrap();
}
