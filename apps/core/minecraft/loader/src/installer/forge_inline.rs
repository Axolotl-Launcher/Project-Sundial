// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Converts Forge's inline installer profile to a version with explicit downloads.

use axlauncher_spi::{LoaderError, LoaderFile, LoaderKind, LoaderProfile};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{InstallerArchive, malformed, read_entry};
use crate::library;

const FORGE: LoaderKind = LoaderKind::Forge;
const FORGE_MAVEN: &str = "https://maven.minecraftforge.net";
const MOJANG_MAVEN: &str = "https://libraries.minecraft.net";
const MAVEN_CENTRAL: &str = "https://repo.maven.apache.org/maven2";

#[derive(Deserialize)]
struct InlineProfile {
    install: UniversalJar,
    #[serde(rename = "versionInfo")]
    version: Map<String, Value>,
}

#[derive(Deserialize)]
struct UniversalJar {
    minecraft: String,
    path: String,
    #[serde(rename = "filePath")]
    archive_path: String,
}

pub(super) fn parse(
    archive: &mut InstallerArchive<'_>,
    document: &Value,
    game_version: &str,
) -> Result<LoaderProfile, LoaderError> {
    let InlineProfile {
        install,
        mut version,
    } = InlineProfile::deserialize(document)
        .map_err(|error| malformed(FORGE, format!("an invalid inline installer: {error}")))?;
    if install.minecraft != game_version
        || version
            .get("inheritsFrom")
            .is_some_and(|parent| parent.as_str() != Some(game_version))
    {
        return Err(malformed(
            FORGE,
            format!("an inline installer for a client other than Minecraft {game_version}"),
        ));
    }
    let version_id = version
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| malformed(FORGE, "an inline installer without a version id"))?
        .to_owned();
    version.insert("inheritsFrom".into(), json!(game_version));

    let path = library::path(FORGE, &install.path)?;
    let bytes = read_entry(archive, install.archive_path.trim_start_matches('/'), FORGE)?;
    let embedded = json!({
        "name": install.path,
        "downloads": {"artifact": {"path": path, "url": "", "size": bytes.len()}},
    });
    normalise_libraries(&mut version, &install.path, embedded)?;
    let document = serde_json::to_vec(&version)
        .map_err(|error| malformed(FORGE, format!("an invalid version profile: {error}")))?;
    Ok(LoaderProfile {
        version_id,
        document,
        files: vec![LoaderFile {
            path: format!("libraries/{path}"),
            bytes,
        }],
    })
}

fn normalise_libraries(
    version: &mut Map<String, Value>,
    embedded_name: &str,
    embedded: Value,
) -> Result<(), LoaderError> {
    let libraries = version
        .entry("libraries")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| malformed(FORGE, "an invalid inline library list"))?;
    let mut embedded = Some(embedded);
    for library in libraries.iter_mut() {
        let fields = library
            .as_object_mut()
            .ok_or_else(|| malformed(FORGE, "an invalid inline library"))?;
        let name = fields
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| malformed(FORGE, "an inline library without a name"))?;
        if name == embedded_name {
            *library = embedded
                .take()
                .ok_or_else(|| malformed(FORGE, "a repeated universal JAR library"))?;
        } else {
            prepare_library(name.to_owned(), fields)?;
        }
    }
    if let Some(embedded) = embedded {
        libraries.push(embedded);
    }
    Ok(())
}

fn prepare_library(name: String, fields: &mut Map<String, Value>) -> Result<(), LoaderError> {
    let checksums = fields
        .remove("checksums")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|hash| {
            hash.as_str().is_some_and(|hash| {
                hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        })
        .collect::<Vec<_>>();
    let mut downloads = match fields.remove("downloads") {
        Some(downloads) => downloads,
        None => library_downloads(&name, fields)?,
    };
    let Some(downloads) = downloads.as_object_mut() else {
        return Err(malformed(FORGE, "an invalid inline library download list"));
    };
    if let Some(artifact) = downloads.get_mut("artifact") {
        prepare_artifact(artifact, &checksums)?;
    }
    if let Some(classifiers) = downloads
        .get_mut("classifiers")
        .and_then(Value::as_object_mut)
    {
        for artifact in classifiers.values_mut() {
            prepare_artifact(artifact, &checksums)?;
        }
    }
    fields.remove("url");
    fields.insert("downloads".into(), json!(downloads));
    Ok(())
}

fn library_downloads(name: &str, fields: &Map<String, Value>) -> Result<Value, LoaderError> {
    let repository = repository(name, fields.get("url").and_then(Value::as_str));
    require_https(repository)?;
    let Some(natives) = fields.get("natives").and_then(Value::as_object) else {
        return Ok(json!({"artifact": library::artifact(FORGE, name, repository)?}));
    };
    library::native_downloads(FORGE, name, repository, natives)
}

fn prepare_artifact(artifact: &mut Value, checksums: &[Value]) -> Result<(), LoaderError> {
    if let Some(url) = artifact
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
    {
        require_https(url)?;
        if artifact.get("sha1").and_then(Value::as_str).is_none() && !checksums.is_empty() {
            artifact["sha1_alternatives"] = json!(checksums);
        }
    }
    Ok(())
}

fn require_https(url: &str) -> Result<(), LoaderError> {
    if url.strip_prefix("https://").is_some_and(|rest| {
        !rest.is_empty()
            && !rest.starts_with('/')
            && !rest.contains('\\')
            && !rest.chars().any(char::is_whitespace)
    }) {
        return Ok(());
    }
    Err(malformed(
        FORGE,
        "an inline library with a non-HTTPS download source",
    ))
}

fn repository<'a>(name: &str, stated: Option<&'a str>) -> &'a str {
    let group = name.split(':').next().unwrap_or_default();
    let artifact = name.split(':').nth(1).unwrap_or_default();
    // Forge published these Scala builds under coordinates absent from Central.
    if group == "org.scala-lang"
        && ["scala-parser-combinators_", "scala-swing_", "scala-xml_"]
            .iter()
            .any(|prefix| artifact.starts_with(prefix))
    {
        return FORGE_MAVEN;
    }
    match group {
        "org.ow2.asm" | "org.scala-lang" | "com.typesafe" | "com.typesafe.akka" | "org.jline"
        | "jline" | "net.java.dev.jna" => return MAVEN_CENTRAL,
        _ => (),
    }
    match stated.map(|url| url.trim_end_matches('/')) {
        None | Some("") => MOJANG_MAVEN,
        Some(
            "http://files.minecraftforge.net/maven"
            | "https://files.minecraftforge.net/maven"
            | "http://maven.minecraftforge.net",
        ) => FORGE_MAVEN,
        Some(url) => url,
    }
}
