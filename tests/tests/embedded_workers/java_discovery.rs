use std::time::Duration;

use axlauncher_storage::{DataPath, Document};
use axlauncher_worker::java_discovery::{Candidate, Command};
use axlauncher_worker::{Event, Images, Request, Service};

use super::{Fixture, run_for};

const SCAN_DEADLINE: Duration = Duration::from_secs(90);

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn java_discovery_starts_on_request_and_reaps_before_the_next_scan() {
    let fixture = Fixture::new();
    let worker = Service::<Candidate>::for_executable(
        fixture.images.java_discovery,
        fixture.executable.clone(),
    );
    assert!(worker.running_id().is_none());
    assert!(!fixture.has_extracted_libraries());

    let observe_start = worker.hold();
    let mut first = worker.request(&fixture.data, Command::Scan).unwrap();
    assert!(worker.running_id().is_some());
    assert!(fixture.has_extracted_libraries());
    drop(observe_start);
    run_for(SCAN_DEADLINE, assert_scan(&mut first));
    fixture.wait_for_cleanup();
    assert!(worker.running_id().is_none());

    let mut second = worker.request(&fixture.data, Command::Scan).unwrap();
    run_for(SCAN_DEADLINE, assert_scan(&mut second));
    fixture.wait_for_cleanup();
    assert!(worker.running_id().is_none());
    assert!(
        !Document::JavaCatalog
            .path(fixture.data.path(DataPath::Root))
            .exists()
    );
}

async fn assert_scan(request: &mut Request<Candidate>) {
    loop {
        match request.next().await.unwrap() {
            Event::Reply(candidate) => assert!(candidate.into_path().is_absolute()),
            Event::Finished(result) => return result.unwrap(),
            Event::Disconnected => panic!("the Java discovery worker disconnected"),
        }
    }
}

#[test]
#[ignore = "run node scripts/desktop.ts smoke to supply the packaged desktop executable"]
fn a_java_discovery_image_not_matching_the_embedded_bytes_is_refused() {
    let fixture = Fixture::new();
    let wrong = Images::new(b"", b"", b"not the embedded library", b"", b"");
    let worker =
        Service::<Candidate>::for_executable(wrong.java_discovery, fixture.executable.clone());
    let error = worker.request(&fixture.data, Command::Scan).err().unwrap();
    assert!(error.contains("worker library does not match the embedded image"));
    assert!(!fixture.has_extracted_libraries());
}
