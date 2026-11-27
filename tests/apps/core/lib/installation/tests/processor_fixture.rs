use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

fn main() {
    let mut args = std::env::args_os().skip(1);
    match args.next().unwrap().to_str().unwrap() {
        "-cp" => {
            assert_eq!(args.nth(2).unwrap(), "--output");
            processor(Path::new(&args.next().unwrap()));
        }
        "--axlauncher-worker" => worker(),
        other => panic!("unexpected fixture argument {other}"),
    }
}

fn processor(output: &Path) {
    let mode = fs::read_to_string("processor.mode").unwrap();
    fs::write(output, if mode == "pass" { b"complete" } else { b"partial!" }).unwrap();
    fs::write("processor.pid", std::process::id().to_string()).unwrap();
    if mode == "fail" {
        std::process::exit(27);
    }
    if mode == "hang" {
        std::thread::sleep(std::time::Duration::from_secs(45));
    }
}

fn worker() {
    let exe = std::env::current_exe().unwrap();
    let root = exe.parent().unwrap();
    io::stdout().write_all(b"\x07\0\0\0\"Ready\"").unwrap();
    io::stdout().flush().unwrap();
    let mut input = io::stdin().lock();
    let mut length = [0; 4];
    input.read_exact(&mut length).unwrap();
    let mut frame = vec![0; u32::from_le_bytes(length) as usize];
    input.read_exact(&mut frame).unwrap();
    fs::write(root.join("processor.mode"), "hang").unwrap();
    let mut child = Command::new(exe.as_os_str())
        .args(["-cp", "fixture.jar", "Fixture", "--output", "partial.jar"])
        .current_dir(root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().unwrap();
    child.wait().unwrap();
}
