use super::*;
use crate::Images;
use crate::protocol::{self, Input, Output};

#[tokio::test]
async fn configuring_java_discovery_is_lazy_and_the_application_can_close_it() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    Images::new(b"", b"", b"").configure();

    let worker = WORKER.get().unwrap();
    assert!(worker.running_id().is_none());
    assert!(!data.path(axlauncher_storage::DataPath::Cache).exists());

    Images::begin_shutdown().join().unwrap();
    assert!(worker.closed_and_reaped());
    assert_eq!(scan(&data).await.unwrap_err(), "The worker is closed.");
}

#[test]
fn scan_and_candidate_frames_preserve_native_paths() {
    let input = protocol::encode(&Input::Request {
        id: 4,
        command: Command::Scan,
    })
    .unwrap();
    assert!(matches!(
        protocol::read::<Input<Command>>(&mut input.as_slice()).unwrap(),
        Some(Input::Request {
            id: 4,
            command: Command::Scan
        })
    ));

    let path = non_unicode_path();
    let output = protocol::encode(&Output::Event {
        id: 4,
        reply: Candidate::new(path.clone()),
    })
    .unwrap();
    let Some(Output::Event { id: 4, reply }) =
        protocol::read::<Output<Candidate>>(&mut output.as_slice()).unwrap()
    else {
        panic!("expected a candidate frame");
    };
    assert_eq!(reply.into_path().as_os_str(), path.as_os_str());
}

#[cfg(unix)]
fn non_unicode_path() -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(b"/jdks/\xff/bin/java".to_vec()).into()
}

#[cfg(windows)]
fn non_unicode_path() -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    let units: Vec<_> = "C:\\jdks\\"
        .encode_utf16()
        .chain([0xd800])
        .chain("\\bin\\java.exe".encode_utf16())
        .collect();
    std::ffi::OsString::from_wide(&units).into()
}

#[tokio::test]
async fn candidates_are_sorted_and_deduplicated_only_after_success() {
    let mut events = [
        candidate("z"),
        candidate("a"),
        candidate("a"),
        Event::Finished(Ok(())),
    ]
    .into_iter();
    assert_eq!(
        collect(async || events.next()).await.unwrap(),
        [PathBuf::from("a"), PathBuf::from("z")]
    );
}

#[tokio::test]
async fn failed_cancelled_and_incomplete_scans_discard_streamed_candidates() {
    for error in ["worker failed", "Java discovery cancelled."] {
        let mut events = [candidate("partial"), Event::Finished(Err(error.into()))].into_iter();
        assert_eq!(collect(async || events.next()).await.unwrap_err(), error);
    }
    let mut events = [candidate("partial")].into_iter();
    assert!(
        collect(async || events.next())
            .await
            .unwrap_err()
            .contains("before completing")
    );
}

fn candidate(path: &str) -> Event<Candidate> {
    Event::Reply(Candidate::new(PathBuf::from(path)))
}
