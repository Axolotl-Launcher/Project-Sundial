// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Initializes the language that Minecraft loads from the instance options.

use std::fs;
use std::io;
use std::path::Path;

use axlauncher_domain::normalize_game_language;
use axlauncher_storage::write_atomic;

const LANG: &str = "lang:";
const UNICODE_FONT: &str = "forceUnicodeFont:";
const FIRST_LANGUAGE_SELECTION: &str = "2011-11-18";
const LOWERCASE_REGIONS: &str = "2016-08-10";

/// The preferred language and the base version that determines its spelling.
#[derive(Debug, Clone)]
pub struct GameLanguage {
    minecraft: String,
    preferred: String,
}

impl GameLanguage {
    pub fn new(minecraft: impl Into<String>, preferred: &str) -> Self {
        Self {
            minecraft: minecraft.into(),
            preferred: normalize_game_language(preferred).unwrap_or_default(),
        }
    }

    pub(crate) fn minecraft(&self) -> &str {
        &self.minecraft
    }

    pub(crate) fn apply(&self, release_time: Option<&str>, path: &Path) -> io::Result<()> {
        let Some(date) = release_time.and_then(|time| time.split_once('T').map(|(date, _)| date))
        else {
            return Ok(());
        };
        // Mojang's timestamps start with an ISO calendar date in UTC.
        if date < FIRST_LANGUAGE_SELECTION {
            return Ok(());
        }
        if self.preferred.is_empty() {
            return Ok(());
        }
        let input = read_options(path)?;
        let output = configure(&input, &self.preferred, date < LOWERCASE_REGIONS);
        if output == input {
            return Ok(());
        }
        write_atomic(path, output.as_bytes())
    }
}

fn read_options(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(contents) if contents.contains('\0') => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "options.txt must be UTF-8 text",
        )),
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error),
    }
}

fn configure(input: &str, preferred: &str, uppercase_region: bool) -> String {
    let contents = input.strip_prefix('\u{feff}').unwrap_or(input);
    let previous = option(contents, LANG);
    let existing = previous
        .and_then(normalize_game_language)
        .filter(|code| !code.is_empty());
    let fresh = existing.is_none();
    let language = existing.unwrap_or_else(|| preferred.to_owned());

    let language = match language.split_once('_') {
        Some((code, region)) if uppercase_region => {
            format!("{code}_{}", region.to_ascii_uppercase())
        }
        _ => language,
    };
    let mut output = input.to_owned();
    if previous != Some(&language) {
        output = set_option(&output, LANG, &language);
    }
    if fresh {
        let needs_unicode = matches!(preferred.split('_').next(), Some("zh" | "ja" | "ko"));
        let current = option(contents, UNICODE_FONT);
        let required = if needs_unicode { "true" } else { "false" };
        if current != Some(required) && (needs_unicode || current.is_some()) {
            output = set_option(&output, UNICODE_FONT, required);
        }
    }
    output
}

fn option<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    contents
        .lines()
        .find_map(|line| line.strip_prefix(key).map(str::trim))
}

fn set_option(input: &str, key: &str, value: &str) -> String {
    let (bom, contents) = input
        .strip_prefix('\u{feff}')
        .map_or(("", input), |text| ("\u{feff}", text));
    let newline = if contents.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut output = String::from(bom);
    let mut found = false;
    for line in contents.split_inclusive('\n') {
        if !line.starts_with(key) {
            output.push_str(line);
            continue;
        }
        found = true;
        output.push_str(key);
        output.push_str(value);
        output.push_str(&line[line.trim_end_matches(['\r', '\n']).len()..]);
    }
    if !found {
        if !contents.is_empty() && !contents.ends_with('\n') {
            output.push_str(newline);
        }
        output.push_str(key);
        output.push_str(value);
        output.push_str(newline);
    }
    output
}
