// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axlauncher_launch::JavaRuntime;

use super::*;

const VERSION: &str = "1.21.11";
const MINIMUM_JDK: u32 = 17;
const PROBE_RESULT: &str = "runtime-probe.txt";
const JAVAC: &str = if cfg!(windows) { "javac.exe" } else { "javac" };
const PROGRAM: &str = r#"
import java.nio.file.Files;
import java.nio.file.Path;

class RuntimeProbe {
    public static void main(String[] args) throws Exception {
        Files.writeString(Path.of("runtime-probe.txt"),
            Runtime.version().feature() + "\n" + System.getProperty("os.arch"));
    }
}
"#;

#[test]
#[ignore = "requires a native JDK 17 or newer"]
fn a_real_jvm_runs_through_desktop_java_management() {
    let temporary = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(temporary.path()).unwrap();
    let catalog = java::list(&data).unwrap();
    let mut native = catalog.available().iter().filter(|runtime| {
        runtime.major() >= MINIMUM_JDK
            && runtime.architecture().is_some_and(|architecture| {
                native_architecture(architecture) == std::env::consts::ARCH
            })
    });
    let installed = native
        .clone()
        .find(|runtime| compiler(runtime).is_file())
        .expect("the host must have a discoverable native JDK 17 or newer");
    let major = installed.major();
    let automatic = catalog.default_for(major).unwrap();
    let explicit = native
        .find(|runtime| runtime.major() != major)
        .unwrap_or(installed);
    let entry = prepare_probe(&data, installed);
    let job = || LaunchJob::for_profile(&data, &entry, "Player".into(), None).unwrap();

    assert_selected_java(&job(), automatic);
    run_probe(job(), major);

    let added = java::add(&data, explicit.path()).unwrap();
    java::set_preferred(&data, added.major(), Some(added.path())).unwrap();
    java::set_profile_preference(&data, entry.profile.id(), Some(added.path())).unwrap();
    let saved = java::list(&data).unwrap();
    assert_eq!(saved.manual_paths().collect::<Vec<_>>(), [added.path()]);
    assert_eq!(saved.default_for(added.major()), Some(added.path()));
    assert_eq!(
        saved.profile_preference(entry.profile.id()),
        Some(added.path())
    );
    assert_selected_java(&job(), added.path());
    run_probe(job(), added.major());

    assert!(java::remove(&data, added.path()).unwrap());
    let restored = java::list(&data).unwrap();
    assert_eq!(restored.manual_paths().count(), 0);
    assert_eq!(restored.preferred().count(), 0);
    assert!(restored.profile_preference(entry.profile.id()).is_none());
    assert_selected_java(&job(), automatic);
    println!(
        "{} {}: ran automatic Java {major} and explicit Java {}, then verified removal",
        std::env::consts::OS,
        std::env::consts::ARCH,
        explicit.full_version()
    );
}

fn prepare_probe(data: &DataDir, installed: &JavaRuntime) -> DiscoveredProfile {
    profile::create(&data.instances(), "Native Java probe", Game::new(VERSION)).unwrap();
    let entry = profile::discover(&data.instances()).instances.remove(0);
    let document = data.game().version_document(VERSION);
    fs::create_dir_all(document.parent().unwrap()).unwrap();
    let manifest = probe_manifest(installed.major());
    fs::write(document, manifest).unwrap();
    fs::write(
        data.game().client_jar(VERSION),
        b"native Java probe fixture",
    )
    .unwrap();
    compile_probe(&entry.directory, installed);
    entry
}

fn probe_manifest(major: u32) -> String {
    format!(
        r#"{{"id":"{VERSION}","javaVersion":{{"majorVersion":{major}}},
             "downloads":{{"client":{{}}}},"mainClass":"RuntimeProbe",
             "arguments":{{"jvm":["-cp","."]}}}}"#
    )
}

fn compiler(runtime: &JavaRuntime) -> PathBuf {
    runtime.path().with_file_name(JAVAC)
}

fn compile_probe(directory: &Path, runtime: &JavaRuntime) {
    let source = "RuntimeProbe.java";
    let release = MINIMUM_JDK.to_string();
    fs::write(directory.join(source), PROGRAM).unwrap();
    let output = Command::new(compiler(runtime))
        .current_dir(directory)
        .args(["--release", release.as_str(), source])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "javac failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_probe(job: LaunchJob, major: u32) {
    let result = job.instance_directory().join(PROBE_RESULT);
    if result.is_file() {
        fs::remove_file(&result).unwrap();
    }
    let started = Arc::new(AtomicBool::new(false));
    let reported = Arc::clone(&started);
    job.run(
        Player::<SystemCredentials>::Offline(Session::offline("Player")),
        DownloadOptions::from_settings(None),
        |_| panic!("offline probe unexpectedly resolved an account"),
        move || reported.store(true, Ordering::Relaxed),
    )
    .expect("the desktop launch must execute the probe on the real JVM");
    assert!(started.load(Ordering::Relaxed));
    let output = fs::read_to_string(result).unwrap();
    let (version, architecture) = output.split_once('\n').unwrap();
    assert_eq!(version.parse::<u32>().unwrap(), major);
    assert_eq!(native_architecture(architecture), std::env::consts::ARCH);
}

fn native_architecture(jvm: &str) -> &str {
    match jvm {
        "amd64" => "x86_64",
        "arm64" => "aarch64",
        "i386" | "i686" => "x86",
        other => other,
    }
}
