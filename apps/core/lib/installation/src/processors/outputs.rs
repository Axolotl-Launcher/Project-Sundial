use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::PathBuf;

use axlauncher_launch::version::{Processor, Version};
use axlauncher_storage::GameDir;

use super::{ProcessorError, resolve_template};

pub(super) struct Output {
    path: PathBuf,
    sha: Option<String>,
}

impl Output {
    pub fn current(&self, require_digest: bool) -> bool {
        match &self.sha {
            Some(sha) => fs::File::open(&self.path).is_ok_and(|file| {
                axlauncher_network::verify_reader(file, sha).is_ok_and(|valid| valid)
            }),
            None => !require_digest && self.path.is_file(),
        }
    }

    pub fn verify(&self) -> Result<(), ProcessorError> {
        self.current(false)
            .then_some(())
            .ok_or_else(|| self.error("processor output is missing or incomplete"))
    }

    /// Removes stale outputs only from inside the game tree, without following the final symlink.
    pub fn withdraw(&self, game: &GameDir) -> Result<(), ProcessorError> {
        match fs::symlink_metadata(&self.path) {
            Ok(_) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(self.error(error)),
        }
        let root = fs::canonicalize(game.root()).map_err(|error| self.error(error))?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| self.error("no output parent"))?;
        let parent = fs::canonicalize(parent).map_err(|error| self.error(error))?;
        if !parent.starts_with(root) {
            return Err(self.error("processor output leaves the game tree"));
        }
        fs::remove_file(&self.path).map_err(|error| self.error(error))
    }

    fn error(&self, reason: impl ToString) -> ProcessorError {
        ProcessorError::Output {
            path: self.path.clone(),
            reason: reason.to_string(),
        }
    }
}

pub(super) fn output_paths(
    processor: &Processor,
    game: &GameDir,
    version: &Version,
    values: &[(&str, OsString)],
) -> Result<Vec<Output>, ProcessorError> {
    declared_outputs(processor)
        .into_iter()
        .map(|(template, sha)| {
            let resolve = |template| resolve_template(template, game.libraries(), version, values);
            let path = game.root().join(resolve(template)?);
            let sha = sha
                .and_then(|sha| resolve(sha).ok()?.into_string().ok())
                .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()));
            Ok(Output { path, sha })
        })
        .collect()
}

fn declared_outputs(processor: &Processor) -> Vec<(&str, Option<&str>)> {
    if !processor.outputs.is_empty() {
        return processor
            .outputs
            .iter()
            .map(|(path, sha)| (path.as_str(), Some(sha.as_str())))
            .collect();
    }
    if !processor
        .jar
        .split(':')
        .nth(1)
        .is_some_and(|name| name.eq_ignore_ascii_case("binarypatcher"))
    {
        return Vec::new();
    }
    processor
        .args
        .windows(2)
        .filter(|pair| pair[0] == "--output")
        .map(|pair| (pair[1].as_str(), None))
        .collect()
}
