// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

use axlauncher_spi::JavaMode;

use crate::platform::{name, sibling};

use super::release::JavaRelease;

/// A locally inspected Java installation identified by its executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaRuntime {
    executable: PathBuf,
    full_version: String,
    version: JavaRelease,
    architecture: Option<String>,
}

impl JavaRuntime {
    /// Reads installation metadata without starting an executable.
    pub fn inspect(path: &Path) -> Option<Self> {
        let target = dunce::canonicalize(path).ok()?;
        let file = target.file_name()?.to_str()?;
        if ![name(JavaMode::Console), name(JavaMode::Windowed)]
            .iter()
            .any(|name| name.eq_ignore_ascii_case(file))
        {
            return None;
        }
        let executable = sibling(&target, JavaMode::Console).unwrap_or(target);
        if !is_executable(&executable) {
            return None;
        }
        let release = read_release(&executable)?;
        let reported_version = property(&release, "JAVA_VERSION")?;
        let (full_version, version) = property(&release, "JAVA_RUNTIME_VERSION")
            .into_iter()
            .chain([reported_version])
            .find_map(|value| Some((value, JavaRelease::parse(value)?)))?;
        Some(Self {
            executable,
            full_version: full_version.to_owned(),
            version,
            architecture: property(&release, "OS_ARCH").map(str::to_owned),
        })
    }

    pub fn path(&self) -> &Path {
        &self.executable
    }

    pub fn major(&self) -> u32 {
        self.version.major()
    }

    pub fn full_version(&self) -> &str {
        &self.full_version
    }

    pub fn architecture(&self) -> Option<&str> {
        self.architecture.as_deref()
    }

    pub(super) fn compare_release(&self, other: &Self) -> Ordering {
        self.version.cmp(&other.version)
    }

    pub(super) fn executable(&self, mode: JavaMode) -> PathBuf {
        sibling(&self.executable, mode)
            .filter(|path| is_executable(path))
            .unwrap_or_else(|| self.executable.clone())
    }
}

fn read_release(executable: &Path) -> Option<String> {
    let home = executable.parent()?.parent()?;
    if let Ok(release) = fs::read_to_string(home.join("release")) {
        return Some(release);
    }
    // Java 8 JDKs can expose the bundled JRE's executable on PATH.
    if home.file_name()? != "jre" {
        return None;
    }
    fs::read_to_string(home.parent()?.join("release")).ok()
}

fn property<'a>(release: &'a str, name: &str) -> Option<&'a str> {
    release.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (name == key).then(|| value.trim().trim_matches('"'))
    })
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    metadata.is_file()
}
