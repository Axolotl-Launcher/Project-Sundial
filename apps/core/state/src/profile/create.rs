// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use axlauncher_storage::AtomicFile;
use uuid::Uuid;

use super::{FILE_NAME, Game, SCHEMA_VERSION, visible_name};

/// Why a blank instance could not be saved.
#[derive(Debug)]
pub enum CreateError {
    InvalidName,
    InvalidVersion,
    Write { path: PathBuf, source: io::Error },
}

impl fmt::Display for CreateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => formatter.write_str("Enter an instance name using visible text."),
            Self::InvalidVersion => {
                formatter.write_str("Enter a Minecraft version ID such as 1.21.11.")
            }
            Self::Write { path, source } => {
                write!(formatter, "Cannot save {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for CreateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Write { source, .. } => Some(source),
            Self::InvalidName | Self::InvalidVersion => None,
        }
    }
}

/// A new managed profile directory with no downloaded files or content.
///
/// A game names the Minecraft version and, when it has one, the loader the
/// profile starts with. Omitting it leaves the game unconfigured.
pub fn create_blank(
    instances: &Path,
    name: &str,
    game: Option<Game>,
) -> Result<PathBuf, CreateError> {
    let name = name.trim();
    if !visible_name(name) {
        return Err(CreateError::InvalidName);
    }
    let game = match game {
        Some(mut game) => {
            let minecraft = game.minecraft.trim();
            if minecraft.is_empty() || !valid_version(minecraft) {
                return Err(CreateError::InvalidVersion);
            }
            game.minecraft = minecraft.to_owned();
            Some(game)
        }
        None => None,
    };
    let id = Uuid::new_v4().to_string();
    let directory = instances.join(&id);
    let profile = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "id": id,
        "instance": { "name": name },
        "game": game,
    });
    let path = directory.join(FILE_NAME);
    let bytes =
        serde_json::to_vec_pretty(&profile).map_err(|source| write_error(&path, source.into()))?;
    fs::create_dir_all(instances).map_err(|source| write_error(instances, source))?;
    fs::create_dir(&directory).map_err(|source| write_error(&directory, source))?;
    if let Err(source) = write_manifest(&path, &bytes) {
        let _ = fs::remove_dir(&directory);
        return Err(write_error(&path, source));
    }
    Ok(directory)
}

fn write_manifest(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut staged = AtomicFile::new(path)?;
    staged.file().write_all(bytes)?;
    staged.commit(path)
}

fn valid_version(version: &str) -> bool {
    !version.ends_with('.')
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
}

fn write_error(path: &Path, source: io::Error) -> CreateError {
    CreateError::Write {
        path: path.to_path_buf(),
        source,
    }
}
