// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use axlauncher_core::launch::Version;
use axlauncher_core::launch::version::{Artifact, JavaVersion, Library, LibraryDownloads};
use axlauncher_modpack::MultiMcExport;
use serde::Deserialize;

use crate::artifact::LocalArtifact;
use crate::{Result, jars};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Patch {
    #[serde(default = "current_format")]
    format_version: u32,
    uid: String,
    main_class: Option<String>,
    minecraft_arguments: Option<String>,
    compatible_java_majors: Option<Vec<u32>>,
    #[serde(default)]
    libraries: Vec<LocalArtifact>,
    main_jar: Option<LocalArtifact>,
    #[serde(default)]
    jar_mods: Vec<LocalArtifact>,
    #[serde(default)]
    requires: Vec<Dependency>,
    #[serde(default)]
    conflicts: Vec<Dependency>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

fn current_format() -> u32 {
    1
}

#[derive(Deserialize)]
struct Dependency {
    uid: String,
    equals: Option<String>,
}

impl Dependency {
    fn matches(&self, export: &MultiMcExport) -> bool {
        export
            .component_version(&self.uid)
            .is_some_and(|version| self.equals.as_deref().is_none_or(|exact| exact == version))
    }
}

pub(super) fn read(export: &mut MultiMcExport) -> Result<Vec<Patch>> {
    let mut patches = Vec::new();
    export.read_patches(|uid, bytes| {
        let patch: Patch =
            serde_json::from_slice(bytes).map_err(|error| invalid(format!("{uid}: {error}")))?;
        patch.validate(uid)?;
        patches.push(patch);
        Ok(())
    })?;
    for patch in &patches {
        patch.validate_dependencies(export)?;
    }
    Ok(patches)
}

impl Patch {
    fn validate(&self, uid: &str) -> io::Result<()> {
        if self.uid != uid || self.format_version != 1 {
            return Err(invalid(format!(
                "{uid}: incompatible patch UID or format version"
            )));
        }
        if let Some(field) = self.extra.keys().find(|field| {
            !matches!(
                field.as_str(),
                "name" | "version" | "order" | "releaseTime" | "type" | "volatile"
            )
        }) {
            return Err(invalid(format!(
                "{uid}: unsupported MultiMC patch field {field}"
            )));
        }
        if self
            .main_class
            .as_deref()
            .is_some_and(|value| !value.split('.').all(java_identifier))
        {
            return Err(invalid(format!("{uid}: invalid main class")));
        }
        if let Some(majors) = &self.compatible_java_majors
            && !matches!(majors.as_slice(), [major] if *major > 0)
        {
            return Err(invalid(format!("{uid}: a patch must pin one Java major")));
        }
        Ok(())
    }

    fn validate_dependencies(&self, export: &MultiMcExport) -> io::Result<()> {
        let invalid_dependency = self
            .requires
            .iter()
            .find(|dependency| !dependency.matches(export));
        let conflict = self
            .conflicts
            .iter()
            .find(|conflict| conflict.matches(export));
        if let Some(dependency) = invalid_dependency.or(conflict) {
            return Err(invalid(format!(
                "{}: incompatible MultiMC component {}",
                self.uid, dependency.uid
            )));
        }
        Ok(())
    }

    fn apply_arguments(&self, version: &mut Version) -> io::Result<()> {
        if let Some(main_class) = &self.main_class {
            version.main_class.clone_from(main_class);
        }
        if self.minecraft_arguments.is_some() {
            replace_arguments(version, &self.minecraft_arguments, &self.uid)?;
        }
        if let Some(majors) = &self.compatible_java_majors {
            version.java_version = Some(JavaVersion { major: majors[0] });
        }
        Ok(())
    }
}

fn java_identifier(part: &str) -> bool {
    let mut chars = part.chars();
    let special = |character: char| matches!(character, '_' | '$');
    chars
        .next()
        .is_some_and(|character| character.is_alphabetic() || special(character))
        && chars.all(|character| character.is_alphanumeric() || special(character))
}

fn replace_arguments(
    version: &mut Version,
    arguments: &Option<String>,
    uid: &str,
) -> io::Result<()> {
    if version
        .arguments
        .as_ref()
        .is_some_and(|args| !args.game.is_empty())
    {
        return Err(invalid(format!(
            "{uid}: cannot replace structured game arguments"
        )));
    }
    version.minecraft_arguments.clone_from(arguments);
    Ok(())
}

pub(super) fn apply(
    patches: Vec<Patch>,
    export: &mut MultiMcExport,
    root: &Path,
    version: &mut Version,
) -> Result<()> {
    let work = tempfile::tempdir_in(root)?;
    let mut jars = jars::Overlay::new(work.path());
    for patch in patches {
        patch.apply_arguments(version)?;
        install_libraries(patch.libraries, export, root, version)?;
        jars.add(export, patch.main_jar, patch.jar_mods)?;
    }
    jars.install(export, root, version)
}

fn install_libraries(
    libraries: Vec<LocalArtifact>,
    export: &mut MultiMcExport,
    root: &Path,
    version: &mut Version,
) -> Result<()> {
    for library in libraries {
        install_library(&library, export, root, version)?;
    }
    Ok(())
}

fn install_library(
    library: &LocalArtifact,
    export: &mut MultiMcExport,
    root: &Path,
    version: &mut Version,
) -> Result<()> {
    let coordinate = |name: &str| {
        let mut parts = name.split('@').next().unwrap_or_default().split(':');
        let group = parts.next().unwrap_or_default();
        let artifact = parts.next().unwrap_or_default();
        let classifier = parts.nth(1).unwrap_or_default();
        format!("{group}:{artifact}:{classifier}")
    };
    let key = coordinate(library.name());
    if version
        .libraries
        .iter()
        .any(|item| coordinate(&item.name) == key)
    {
        return Err(format!("MultiMC patch replaces existing library {}", library.name()).into());
    }
    let path = library.maven_path()?;
    let destination = root.join("libraries").join(&path);
    if destination.exists() {
        return Err(format!(
            "MultiMC library destination {} already exists",
            path.display()
        )
        .into());
    }
    let size = library.copy(export, "libraries", &destination)?;
    let artifact = Artifact {
        path: path.to_string_lossy().replace('\\', "/"),
        sha1: None,
        sha1_alternatives: Vec::new(),
        size: Some(size),
        url: Some(String::new()),
    };
    version.libraries.push(Library {
        name: library.name().to_owned(),
        downloads: Some(LibraryDownloads {
            artifact: Some(artifact),
            classifiers: None,
        }),
        natives: None,
        rules: None,
        url: None,
        include_in_classpath: true,
    });
    Ok(())
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
#[path = "../../../tests/tools/mmc-convert/src/patches.rs"]
mod tests;
