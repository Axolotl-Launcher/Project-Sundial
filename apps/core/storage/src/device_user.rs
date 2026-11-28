//! A pseudonymous identifier for the operating system user on this installation.

use std::fmt::Write as _;
use std::{fs, io, path::Path};

use axlauncher_storage_layout::{Document, device_user_config};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, get_current_pid};
use uuid::Uuid;

use crate::write_atomic;

/// The account namespace for the current OS user on this installation.
pub struct DeviceUserId(String);

impl DeviceUserId {
    pub fn current() -> io::Result<Self> {
        Self::resolve(
            process_principal().as_deref(),
            machine_id().as_deref(),
            device_user_config().as_deref(),
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
        let cached = config
            .ok_or_else(unavailable)
            .and_then(|config| Binding::resolve(&config.join(scope), native.as_deref()));
        cached.or_else(|error| native.map(Self).ok_or(error))
    }
}

fn unavailable() -> io::Error {
    io::Error::other("Could not identify the current device user.")
}

fn digest(kind: &str, value: &str) -> String {
    let input = format!("axlauncher:{kind}:v1\0{}\0{value}", std::env::consts::OS);
    hex::encode(Sha256::digest(input))
}

#[derive(Deserialize, Serialize)]
struct Binding {
    namespace: String,
    native: Option<String>,
}

impl Binding {
    fn resolve(directory: &Path, native: Option<&str>) -> io::Result<DeviceUserId> {
        let path = Document::DeviceUser.path(directory);
        let cached = Self::read(&path)?;
        if let Some(cached) = cached.filter(|cached| cached.matches(native)) {
            return Ok(DeviceUserId(cached.namespace));
        }
        let _lock = Document::DeviceUser.lock(directory)?;
        let namespace = match Self::read(&path)? {
            Some(cached) if cached.matches(native) => return Ok(DeviceUserId(cached.namespace)),
            Some(cached) if cached.native.is_none() => cached.namespace,
            _ => native.map_or_else(
                || digest("local-installation", &Uuid::new_v4().to_string()),
                str::to_owned,
            ),
        };
        let binding = Self {
            namespace,
            native: native.map(str::to_owned),
        };
        let bytes = serde_json::to_vec(&binding).expect("device-user binding serializes");
        write_atomic(&path, &bytes)?;
        Ok(DeviceUserId(binding.namespace))
    }

    fn read(path: &Path) -> io::Result<Option<Self>> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let binding: Self = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        if !is_digest(&binding.namespace)
            || binding
                .native
                .as_deref()
                .is_some_and(|value| !is_digest(value))
        {
            return Err(unavailable());
        }
        Ok(Some(binding))
    }

    fn matches(&self, native: Option<&str>) -> bool {
        native.is_none() || self.native.as_deref() == native
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
