//! A pseudonymous identifier for the operating system user on this installation.

use std::fmt::Write as _;
use std::io::Write as _;
use std::{fs, io, path::Path};

use sha2::{Digest, Sha256};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, get_current_pid};
use uuid::Uuid;

use crate::AtomicFile;

/// The account namespace for the current OS user on this installation.
pub struct DeviceUserId(String);

impl DeviceUserId {
    pub fn current() -> io::Result<Self> {
        let config =
            dirs::config_local_dir().map(|path| path.join("AXLauncher").join("device-users"));
        Self::resolve(
            process_principal().as_deref(),
            machine_id().as_deref(),
            config.as_deref(),
        )
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn resolve(
        principal: Option<&str>,
        machine: Option<&str>,
        config: Option<&Path>,
    ) -> io::Result<Self> {
        let native = principal
            .zip(machine)
            .map(|(principal, machine)| digest("device-user", &format!("{machine}\0{principal}")));
        let scope = principal.map_or_else(
            || "unknown".to_owned(),
            |principal| digest("os-principal", principal),
        );
        let binding = config
            .ok_or_else(unavailable)
            .and_then(|config| installation_namespace(&config.join(scope), native.as_deref()));
        binding.or_else(|error| native.map(Self).ok_or(error))
    }
}

fn unavailable() -> io::Error {
    io::Error::other("Could not identify the current device user.")
}

fn digest(kind: &str, value: &str) -> String {
    let input = format!("axlauncher:{kind}:v1\0{}\0{value}", std::env::consts::OS);
    hex::encode(Sha256::digest(input))
}

fn installation_namespace(directory: &Path, native: Option<&str>) -> io::Result<DeviceUserId> {
    let Some(native) = native else {
        return load_or_create(&directory.join("device-user-id"), None);
    };
    let anchor = load_or_create(&directory.join("native-device-id"), Some(native))?;
    if anchor.as_str() == native {
        return load_or_create(&directory.join("device-user-id"), Some(native));
    }
    Ok(DeviceUserId(native.to_owned()))
}

fn load_or_create(path: &Path, seed: Option<&str>) -> io::Result<DeviceUserId> {
    match read_digest(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => initialize(path, seed),
        result => result,
    }
}

fn read_digest(path: &Path) -> io::Result<DeviceUserId> {
    let digest = fs::read_to_string(path)?;
    if is_digest(&digest) {
        return Ok(DeviceUserId(digest));
    }
    Err(unavailable())
}

fn initialize(path: &Path, seed: Option<&str>) -> io::Result<DeviceUserId> {
    let value = seed.map_or_else(
        || digest("local-installation", &Uuid::new_v4().to_string()),
        str::to_owned,
    );
    let mut pending = AtomicFile::new(path)?;
    pending.file().write_all(value.as_bytes())?;
    match pending.commit_new(path) {
        Ok(()) => Ok(DeviceUserId(value)),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => read_digest(path),
        Err(error) => Err(error),
    }
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
    write!(&mut value, "{}", **principal).ok()?;
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
    ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .into_iter()
        .find_map(|path| normalize_machine_id(&fs::read_to_string(path).ok()?))
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
    normalize_machine_id(line.trim().trim_matches('"'))
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
