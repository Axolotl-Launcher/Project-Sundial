use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(15);

pub fn compile(root: &Path) -> PathBuf {
    let source = root.join("processor_fixture.rs");
    std::fs::write(&source, include_str!("processor_fixture.rs")).unwrap();
    let executable = root.join(format!("java{}", std::env::consts::EXE_SUFFIX));
    let result = Command::new("rustc")
        .arg("--edition=2024")
        .arg(source)
        .args(["-o"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    executable
}

pub fn wait_pid(root: &Path) -> u32 {
    let start = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(root.join("processor.pid"))
            && let Ok(pid) = text.parse()
        {
            return pid;
        }
        assert!(start.elapsed() < DEADLINE, "processor did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn wait_dead(pid: u32) -> bool {
    let start = Instant::now();
    while alive(pid) {
        if start.elapsed() >= DEADLINE {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    }
    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/fi", &format!("PID eq {pid}"), "/nh"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).lines().any(|line| {
            line.split_whitespace()
                .nth(1)
                .is_some_and(|column| column.parse() == Ok(pid))
        })
    }
}
