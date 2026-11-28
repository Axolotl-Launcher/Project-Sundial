// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop access to Modrinth project images.

use std::time::Duration;

use super::on_worker;

const IMAGE_TIMEOUT: Duration = Duration::from_secs(15);

/// Downloads an image for a project description. Call from a background task.
pub fn project_image(url: &str) -> Result<Vec<u8>, String> {
    if !axlauncher_network::modrinth::is_project_image_url(url) {
        return Err("The image is outside the Modrinth CDN".into());
    }
    on_worker(async {
        tokio::time::timeout(
            IMAGE_TIMEOUT,
            axlauncher_network::modrinth::project_image(url),
        )
        .await
        .map_err(|_| "The project image request timed out".to_owned())?
        .map_err(|error| error.to_string())
    })
    .map_err(|error| error.to_string())?
}
