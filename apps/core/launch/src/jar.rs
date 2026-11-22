// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Reading jar entries with the `zip` decoder.
//!
//! This is the production [`NativeReader`]: it opens a jar, walks its entries in
//! archive order, and hands their bytes to the extractor. A file that is not a
//! zip archive is reported as such rather than surfacing a decoder message, so a
//! damaged download reads differently from an unreadable archive.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::natives::{NativeError, NativeReader};

/// Reads entries from real jar files.
#[derive(Debug, Default, Clone, Copy)]
pub struct JarReader;

impl NativeReader for JarReader {
    fn read_jar(&self, path: &Path) -> Result<Vec<(String, Vec<u8>)>, NativeError> {
        let file = File::open(path).map_err(|source| NativeError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        let mut archive = zip::ZipArchive::new(file).map_err(|_| NativeError::NotAJar {
            path: path.to_path_buf(),
        })?;

        read_entries(&mut archive, path)
    }
}

fn read_entries<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    path: &Path,
) -> Result<Vec<(String, Vec<u8>)>, NativeError> {
    let mut entries = Vec::with_capacity(archive.len());

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| zip_error(path, &error))?;
        if file.is_dir() {
            continue;
        }
        let name = file
            .name()
            .map_err(|error| zip_error(path, &error))?
            .into_owned();

        let mut bytes = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
        file.read_to_end(&mut bytes)
            .map_err(|source| NativeError::Io {
                path: PathBuf::from(path),
                source,
            })?;
        entries.push((name, bytes));
    }

    Ok(entries)
}

fn zip_error(path: &Path, error: &zip::result::ZipError) -> NativeError {
    NativeError::Zip {
        path: path.to_path_buf(),
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::natives::{NativeEntry, extract};
    use std::fs;
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("axlauncher-jar-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path).expect("temp directory");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Writes a real zip archive holding `files`.
    fn write_jar(path: &Path, files: &[(&str, &[u8])]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        let file = File::create(path).expect("jar file");
        let mut writer = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, '_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

        for (name, bytes) in files {
            writer.start_file(*name, options).expect("start entry");
            writer.write_all(bytes).expect("write entry");
        }
        writer.finish().expect("finish archive");
    }

    #[test]
    fn a_real_jar_yields_its_entries_in_archive_order() {
        let temp = TempDir::new();
        let jar = temp.0.join("natives.jar");
        write_jar(
            &jar,
            &[
                ("a.dll", b"one"),
                ("b/c.so", b"two"),
                ("readme.txt", b"three"),
            ],
        );

        let entries = JarReader.read_jar(&jar).expect("read jar");
        let names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();

        assert_eq!(names, ["a.dll", "b/c.so", "readme.txt"]);
        assert_eq!(entries[0].1, b"one");
        assert_eq!(entries[1].1, b"two");
    }

    #[test]
    fn directory_entries_are_omitted() {
        let temp = TempDir::new();
        let jar = temp.0.join("dirs.jar");

        {
            let file = File::create(&jar).expect("jar file");
            let mut writer = zip::ZipWriter::new(file);
            let options: zip::write::FileOptions<'_, '_, ()> = zip::write::FileOptions::default();
            writer.add_directory("folder/", options).expect("directory");
            writer.start_file("folder/lib.so", options).expect("entry");
            writer.write_all(b"native").expect("bytes");
            writer.finish().expect("finish");
        }

        let names: Vec<String> = JarReader
            .read_jar(&jar)
            .expect("read jar")
            .into_iter()
            .map(|(name, _)| name)
            .collect();

        assert_eq!(names, ["folder/lib.so"]);
    }

    #[test]
    fn extraction_from_a_real_jar_copies_only_natives() {
        let temp = TempDir::new();
        let jars_root = temp.0.join("libraries");
        let jar = jars_root.join("natives.jar");
        write_jar(
            &jar,
            &[("lwjgl.dll", b"dll"), ("META-INF/MANIFEST.MF", b"manifest")],
        );
        let destination = temp.0.join("natives");

        let extracted = extract(
            &JarReader,
            &jars_root,
            &[NativeEntry {
                path: "natives.jar".into(),
            }],
            &destination,
        )
        .expect("extraction succeeds");

        assert_eq!(extracted.files, vec![destination.join("lwjgl.dll")]);
        assert_eq!(
            fs::read(destination.join("lwjgl.dll")).expect("native"),
            b"dll"
        );
    }

    #[test]
    fn a_file_that_is_not_a_zip_is_reported_as_such() {
        let temp = TempDir::new();
        let jar = temp.0.join("not-a-jar.jar");
        fs::write(&jar, b"this is plain text, not an archive").expect("file");

        let error = JarReader.read_jar(&jar).expect_err("must fail");

        assert!(matches!(error, NativeError::NotAJar { .. }));
        assert!(error.to_string().contains("not-a-jar.jar"));
    }

    #[test]
    fn a_missing_jar_reports_the_io_error_and_path() {
        let temp = TempDir::new();
        let jar = temp.0.join("absent.jar");

        let error = JarReader.read_jar(&jar).expect_err("must fail");

        assert!(matches!(error, NativeError::Io { .. }));
        assert!(error.to_string().contains("absent.jar"));
    }
}
