//! A pseudonymous identifier for the operating system user on this installation.

use std::fmt::Write as _;
use std::io;

use sha2::{Digest, Sha256};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, get_current_pid};

/// An account namespace derived from the current process's OS principal and machine ID.
pub struct DeviceUserId(String);

impl DeviceUserId {
    pub fn current() -> io::Result<Self> {
        let principal = process_principal().ok_or_else(unavailable)?;
        let machine = machine_id().ok_or_else(unavailable)?;
        let input = format!(
            "axlauncher:device-user:v1\0{}\0{machine}\0{principal}",
            std::env::consts::OS
        );
        Ok(Self(hex::encode(Sha256::digest(input))))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn unavailable() -> io::Error {
    io::Error::other("Could not identify the current device user.")
}

fn process_principal() -> Option<String> {
    let pid = get_current_pid().ok()?;
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        ProcessRefreshKind::new().with_user(UpdateKind::Always),
    );
    let process = system.process(pid)?;
    #[cfg(windows)]
    let principal = process.user_id()?;
    #[cfg(not(windows))]
    let principal = process.effective_user_id()?;
    let mut value = String::new();
    write!(&mut value, "{}", &**principal).ok()?;
    Some(value)
}

#[cfg(windows)]
fn machine_id() -> Option<String> {
    let key = windows_registry::LOCAL_MACHINE
        .open(r"SOFTWARE\Microsoft\Cryptography")
        .ok()?;
    normalize_machine_id(&key.get_string("MachineGuid").ok()?)
}

#[cfg(target_os = "linux")]
fn machine_id() -> Option<String> {
    use std::fs;

    let value = match fs::read_to_string("/etc/machine-id") {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::read_to_string("/var/lib/dbus/machine-id")
        }
        result => result,
    };
    normalize_machine_id(&value.ok()?)
}

#[cfg(target_os = "macos")]
fn machine_id() -> Option<String> {
    let output = std::process::Command::new("/usr/sbin/ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let line = std::str::from_utf8(&output.stdout)
        .ok()?
        .lines()
        .find_map(|line| {
            line.split_once("\"IOPlatformUUID\" = ")
                .map(|(_, value)| value)
        })?;
    normalize_machine_id(line.trim_matches('"'))
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn machine_id() -> Option<String> {
    None
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn normalize_machine_id(value: &str) -> Option<String> {
    let value = value.trim().replace('-', "");
    (value.len() == 32
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value.bytes().any(|byte| byte != b'0'))
    .then(|| value.to_ascii_lowercase())
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/storage/src/device_user.rs"]
mod tests;
