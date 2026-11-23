// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Preparation of the pinned authlib-injector required for third-party play.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use axlauncher_download_engine::{MissingFile, download_all};
use axlauncher_spi::Network;
use axlauncher_storage::{DataDir, open_for_read};
use sha2::{Digest, Sha256};

const URL: &str = "https://authlib-injector.yushi.moe/artifact/56/authlib-injector-1.2.8.jar";
const SHA256: &str = "9c7f4343e6c82034958ffb48c14a2cb0c85928be7283103ce17da00c6d5a7b10";
const SHA1: &str = "0e0e66d8a4f91a26f33b9c09f5cdffce4a11f0b8";
const SIZE: u64 = 349_681;

/// A verified Java agent in the shared game directory.
pub async fn prepare(network: &impl Network, data: &DataDir) -> Result<PathBuf, String> {
    let path = std::path::absolute(data.auth_agent()).map_err(|error| error.to_string())?;
    if path.to_string_lossy().contains('=') {
        return Err(
            "Choose a launcher data directory without '=' to use third-party accounts.".into(),
        );
    }
    if matches!(is_verified(&path), Ok(true)) {
        return Ok(path);
    }
    let mut results = download_all(
        network,
        &[MissingFile {
            url: URL.to_owned(),
            path: path.clone(),
            sha1: Some(SHA1.to_owned()),
            size: Some(SIZE),
        }],
        1,
    )
    .await;
    results
        .pop()
        .expect("one authentication agent download")
        .result
        .map_err(|error| {
            format!("Could not download the third-party authentication agent: {error}")
        })?;
    if !matches!(is_verified(&path), Ok(true)) {
        return Err("The third-party authentication agent failed SHA-256 verification.".into());
    }
    Ok(path)
}

fn is_verified(path: &Path) -> io::Result<bool> {
    let mut bytes = Vec::new();
    open_for_read(path)?
        .take(SIZE + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes.len() as u64 == SIZE && hex::encode(Sha256::digest(&bytes)) == SHA256)
}
