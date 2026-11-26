// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

use axlauncher_core::launch::Version;
use axlauncher_core::launch::version::{Downloads, GameDownload};
use axlauncher_core::storage::{AtomicFile, GameDir};
use axlauncher_modpack::MultiMcExport;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::Result;
use crate::artifact::LocalArtifact;

pub(super) struct Overlay<'a> {
    work: &'a Path,
    main: Option<LocalArtifact>,
    layers: Vec<PathBuf>,
}

impl<'a> Overlay<'a> {
    pub(super) fn new(work: &'a Path) -> Self {
        Self {
            work,
            main: None,
            layers: Vec::new(),
        }
    }

    pub(super) fn add(
        &mut self,
        export: &mut MultiMcExport,
        main: Option<LocalArtifact>,
        layers: Vec<LocalArtifact>,
    ) -> Result<()> {
        if let Some(main) = main {
            self.main = Some(main);
        }
        for layer in layers {
            let path = self.work.join(format!("layer-{}.jar", self.layers.len()));
            layer.copy(export, "jarmods", &path)?;
            self.layers.push(path);
        }
        Ok(())
    }

    pub(super) fn install(
        self,
        export: &mut MultiMcExport,
        root: &GameDir,
        version: &mut Version,
    ) -> Result<()> {
        if self.main.is_none() && self.layers.is_empty() {
            return Ok(());
        }
        // The chosen replacement is read once after every patch has been applied.
        let base = match self.main {
            Some(main) => {
                let path = self.work.join("replacement.jar");
                main.copy(export, "libraries", &path)?;
                path
            }
            None => root.client_jar(version.jar_id()),
        };
        let destination = root.client_jar(&version.id);
        let size = assemble(&destination, &base, &self.layers)?;
        version.jar = Some(version.id.clone());
        version.downloads = Some(Downloads {
            client: Some(GameDownload {
                sha1: None,
                size: Some(size),
                url: String::new(),
            }),
        });
        Ok(())
    }
}

fn assemble(destination: &Path, base: &Path, layers: &[PathBuf]) -> Result<u64> {
    let mut staged = AtomicFile::new(destination)?;
    let mut jar = ZipWriter::new(staged.file());
    let mut seen = HashSet::new();
    let mut bytes = 0;
    for path in layers.iter().rev() {
        append(&mut jar, path, &mut seen, &mut bytes, false)?;
    }
    append(&mut jar, base, &mut seen, &mut bytes, !layers.is_empty())?;
    let size = jar.finish()?.metadata()?.len();
    staged.commit(destination)?;
    Ok(size)
}

fn append(
    jar: &mut ZipWriter<&mut File>,
    path: &Path,
    seen: &mut HashSet<String>,
    bytes: &mut u64,
    vanilla_base: bool,
) -> Result<()> {
    let mut source = ZipArchive::new(BufReader::new(File::open(path)?))?;
    if source.has_duplicate_names() || source.len() > 100_000 {
        return Err(format!("duplicate or excessive JAR entries in {}", path.display()).into());
    }
    for index in 0..source.len() {
        let file = source.by_index(index)?;
        append_entry(jar, file, seen, bytes, vanilla_base)?;
    }
    Ok(())
}

fn append_entry<R: Read>(
    jar: &mut ZipWriter<&mut File>,
    file: zip::read::ZipFile<'_, R>,
    seen: &mut HashSet<String>,
    bytes: &mut u64,
    vanilla_base: bool,
) -> Result<()> {
    let name = file.name()?.into_owned();
    if file.is_symlink()
        || file.enclosed_name().is_none()
        || name.contains(['\\', ':'])
        || name.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(format!("unsafe JAR entry {name:?}").into());
    }
    if file.is_dir()
        || (vanilla_base && name.starts_with("META-INF/"))
        || !seen.insert(name.clone())
    {
        return Ok(());
    }
    let size = file.size();
    *bytes = bytes.saturating_add(size);
    if *bytes > 1024 * 1024 * 1024 || seen.len() > 100_000 {
        return Err("combined JAR exceeds the size or entry limit".into());
    }
    jar.start_file(name, SimpleFileOptions::default())?;
    if io::copy(&mut file.take(size + 1), jar)? != size {
        return Err("JAR member does not match its declared size".into());
    }
    Ok(())
}
