// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Install and launch actions for profiles shown in the panel.

use std::path::{Path, PathBuf};

use axlauncher_core::install::prepare::Progress;
use axlauncher_core::state::profile::{self, Game, InstallStatus};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::Context;

use crate::installations::{self, Installation, supported_loader, vanilla_version};
use crate::launches::{self, Launch};

use super::RootView;

impl RootView {
    /// A vanilla profile installs its Minecraft version; a loader profile
    /// acquires that version first, resolves the loader's profile, and
    /// prepares the files the loader's document names.
    pub(crate) fn install_game(
        &mut self,
        directory: &Path,
        game: &Game,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(data_root) = self.root.clone() else {
            return;
        };
        let game_root = DataDir::new(&data_root).meta();
        let version = game.minecraft.clone();
        let Some(key) = super::install_key(game) else {
            self.activity.error(format!(
                "Loader {} is not available in this build yet.",
                game.loader
                    .as_ref()
                    .map_or("", |loader| loader.kind.as_str())
            ));
            cx.notify();
            return;
        };
        if matches!(
            self.installation(&game_root, &key),
            Some(Installation::Running(_) | Installation::Complete)
        ) {
            return;
        }
        if let Some(loader) = &game.loader
            && loader.kind != "vanilla"
            && !supported_loader(&loader.kind)
        {
            self.activity.error(format!(
                "Loader {} is not available in this build yet.",
                loader.kind
            ));
            cx.notify();
            return;
        }

        let concurrency = self
            .settings
            .as_ref()
            .map_or(0, |settings| settings.max_concurrent_downloads as usize);
        self.activity
            .info(format!("Installing {version} in {}", game_root.display()));
        self.set_installation(
            &game_root,
            &key,
            Installation::Running(Progress::Version),
            cx,
        );

        let loader = game
            .loader
            .as_ref()
            .filter(|loader| loader.kind != "vanilla");
        match loader {
            Some(_) => installations::start_loader(
                data_root,
                game_root,
                directory.to_path_buf(),
                game.clone(),
                key,
                concurrency,
                cx,
            ),
            None => installations::start(game_root, version, concurrency, cx),
        }
    }

    /// A loader install ends here: the profile records the version id it
    /// produced, so a later launch needs no provider round trip.
    pub(crate) fn finish_loader_install(
        &mut self,
        data_root: &Path,
        directory: &Path,
        key: &str,
        outcome: Result<String, String>,
        cx: &mut Context<'_, Self>,
    ) {
        let game_root = DataDir::new(data_root).meta();
        // The install only counts as complete once the profile records it:
        // Complete hides the Install button, so a profile write failure must
        // surface as a failure the user can retry.
        let persisted = outcome.and_then(|version_id| {
            persist_install(directory, &version_id)?;
            Ok(version_id)
        });
        let outcome = match persisted {
            Ok(version_id) => Ok(version_id),
            Err(error) => {
                let _ = mark_failed(directory);
                Err(error)
            }
        };
        let installation = match outcome {
            Ok(version_id) => {
                self.activity
                    .success(format!("{} installed as {version_id}", directory.display()));
                Installation::Complete
            }
            Err(error) => {
                self.activity.error(error.clone());
                Installation::Failed(error)
            }
        };
        self.set_installation(&game_root, key, installation, cx);
        self.instances.replace(
            profile::discover(&DataDir::new(data_root).instances()),
            true,
        );
        cx.notify();
    }

    fn installation(&self, root: &Path, version: &str) -> Option<&Installation> {
        self.installations.get(root)?.get(version)
    }

    pub(crate) fn set_installation(
        &mut self,
        root: &Path,
        version: &str,
        installation: Installation,
        cx: &mut Context<'_, Self>,
    ) {
        match &installation {
            Installation::Complete => self
                .activity
                .success(format!("{version} ready in {}", root.display())),
            Installation::Failed(error) => self.activity.error(format!(
                "Installing {version} in {} failed: {error}",
                root.display()
            )),
            Installation::Running(_) => (),
        }
        self.installations
            .entry(root.to_path_buf())
            .or_default()
            .insert(version.to_owned(), installation);
        cx.notify();
    }

    pub(crate) fn launch(&mut self, directory: &Path, cx: &mut Context<'_, Self>) {
        let Some(entry) = self.instances.find(directory) else {
            return;
        };
        let Some(request) = self.launch_request(entry, self.player_name(cx)) else {
            return;
        };
        let vanilla = entry
            .profile
            .game
            .as_ref()
            .and_then(vanilla_version)
            .is_some();
        // The install is tracked under the key the row uses, not under the
        // version id a launch asks for: for a loader profile those differ, and
        // launching while its install rewrites the game files must be blocked.
        let installing = entry
            .profile
            .game
            .as_ref()
            .and_then(super::install_key)
            .is_some_and(|key| {
                matches!(
                    self.installation(&request.game_root, &key),
                    Some(Installation::Running(_))
                )
            });
        if self
            .launches
            .get(directory)
            .is_some_and(|(_, launch)| launch.is_active())
            || installing
        {
            return;
        }
        let Some(data_root) = self.root.clone() else {
            return;
        };
        self.set_launch(directory, &request.version_id, Launch::Starting, cx);
        launches::start(request, vanilla, data_root, cx);
    }

    pub(crate) fn set_launch(
        &mut self,
        directory: &Path,
        version: &str,
        launch: Launch,
        cx: &mut Context<'_, Self>,
    ) {
        let message = format!("{} ({version}): {}", directory.display(), launch.message());
        match launch {
            Launch::Starting | Launch::Running => self.activity.info(message),
            Launch::Exited => self.activity.success(message),
            Launch::NeedsInstall(_) | Launch::Failed(_) => self.activity.error(message),
        }
        self.launches
            .insert(directory.to_path_buf(), (version.to_owned(), launch));
        cx.notify();
    }

    /// Forgets a completed install for the profile at `directory`, so a launch
    /// that found files missing offers Install again.
    pub(crate) fn clear_completed_installation(&mut self, root: &Path, directory: &Path) {
        let Some(key) = self
            .instances
            .find(directory)
            .and_then(|entry| entry.profile.game.as_ref())
            .and_then(super::install_key)
        else {
            return;
        };
        if let Some(states) = self.installations.get_mut(root)
            && matches!(states.get(&key), Some(Installation::Complete))
        {
            states.remove(&key);
        }
    }
}

impl RootView {
    /// A new profile gets its own UUID and a directory named after its display
    /// name, made unique under `instances`: two profiles may share a name
    /// without sharing an identity.
    pub(crate) fn create_profile(&mut self, cx: &mut Context<'_, Self>) {
        let Some(data_root) = self.root.clone() else {
            return;
        };
        let name = self.new_name.read(cx).value().trim().to_string();
        let minecraft = self.new_minecraft.read(cx).value().trim().to_string();
        let kind = self.new_loader.read(cx).value().trim().to_ascii_lowercase();
        let version = self.new_loader_version.read(cx).value().trim().to_string();

        if name.is_empty() || minecraft.is_empty() {
            self.activity
                .error("A profile needs a name and a Minecraft version.");
            cx.notify();
            return;
        }
        let mut game = Game::new(minecraft);
        let loader = match loader_choice(&kind, &version) {
            Ok(loader) => loader,
            Err(error) => {
                self.activity.error(error);
                cx.notify();
                return;
            }
        };
        if let Some((kind, version)) = loader {
            game = game.with_loader(kind, version);
        }

        let instances = DataDir::new(&data_root).instances();
        let directory = unique_directory(&instances, &name);
        let profile = profile::Profile::new(name.clone(), Some(game));
        match profile::save(&directory, &profile) {
            Ok(()) => {
                self.activity
                    .success(format!("Created {name} at {}", directory.display()));
                self.instances.replace(profile::discover(&instances), true);
                self.instances.select(&directory);
            }
            Err(error) => self.activity.error(error.to_string()),
        }
        cx.notify();
    }
}

/// Records a successful install on the profile, so a launch needs no provider.
fn persist_install(directory: &Path, version_id: &str) -> Result<(), String> {
    let Some(mut profile) = profile::read(directory).map_err(|error| error.to_string())? else {
        return Err(format!(
            "{} no longer holds a profile document.",
            directory.display()
        ));
    };
    profile.set_installed_version(version_id);
    profile.set_install_status(InstallStatus::Ready);
    profile::save(directory, &profile).map_err(|error| error.to_string())
}

/// Records a failed install so the profile can say so.
fn mark_failed(directory: &Path) -> Result<(), String> {
    let Some(mut profile) = profile::read(directory).map_err(|error| error.to_string())? else {
        return Ok(());
    };
    profile.set_install_status(InstallStatus::Failed);
    profile::save(directory, &profile).map_err(|error| error.to_string())
}

/// The loader a creation form asks for, or nothing for a vanilla profile.
fn loader_choice(kind: &str, version: &str) -> Result<Option<(String, Option<String>)>, String> {
    if kind.is_empty() || kind == "vanilla" {
        return Ok(None);
    }
    if !supported_loader(kind) {
        return Err(format!("Loader {kind} is not available in this build yet."));
    }
    let version = (!version.is_empty()).then(|| version.to_string());
    Ok(Some((kind.to_string(), version)))
}

/// A directory name derived from a display name.
fn slug(name: &str) -> String {
    let mut slug: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "instance".to_string()
    } else {
        slug.to_string()
    }
}

/// A free directory under `instances` for `name`.
fn unique_directory(instances: &Path, name: &str) -> PathBuf {
    let base = slug(name);
    let mut candidate = instances.join(&base);
    let mut attempt = 2;
    while candidate.exists() {
        candidate = instances.join(format!("{base}-{attempt}"));
        attempt += 1;
    }
    candidate
}
