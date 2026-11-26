use std::fs;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

use super::worker_executable;

const REEXEC_PROBE: &str = "AXLAUNCHER_TEST_REEXEC_PROBE";
const TEST_NAME: &str = "bootstrap::tests::can_reexec_after_the_installed_binary_is_replaced";

#[test]
fn can_reexec_after_the_installed_binary_is_replaced() {
    if std::env::var_os(REEXEC_PROBE).is_some() {
        std::io::stdin().read_exact(&mut [0]).unwrap();
        let output = Command::new(worker_executable().unwrap())
            .arg("--list")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains(TEST_NAME)
        );
        return;
    }

    let current = std::env::current_exe().unwrap();
    let root = tempfile::tempdir_in(current.parent().unwrap()).unwrap();
    let executable = root.path().join("installed-binary");
    fs::copy(current, &executable).unwrap();
    let mut child = Command::new(&executable)
        .args(["--exact", TEST_NAME])
        .env(REEXEC_PROBE, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    fs::remove_file(&executable).unwrap();
    fs::write(&executable, b"replacement").unwrap();
    child.stdin.take().unwrap().write_all(&[1]).unwrap();
    let output = child.wait_with_output().unwrap();
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "{report}");
    assert!(report.contains("1 passed"), "{report}");
}
