// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::env;
use std::fs;
use std::path::Path;

const KEY: &str = "CURSEFORGE_API_KEY";

fn main() {
    let manifest_dir =
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo supplies the manifest directory");
    let dotenv = Path::new(&manifest_dir).join("../../../.env");
    println!("cargo::rerun-if-env-changed={KEY}");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed={}", dotenv.display());

    let value = env::var(KEY)
        .ok()
        .and_then(|value| nonempty(&value).map(str::to_owned));
    let dotenv_value = || {
        fs::read_to_string(&dotenv)
            .ok()
            .and_then(|text| read_key(&text).map(str::to_owned))
    };
    if let Some(value) = value.or_else(dotenv_value) {
        assert!(
            value.bytes().all(|byte| byte.is_ascii_graphic()),
            "invalid CurseForge API key value"
        );
        println!("cargo::rustc-env={KEY}={value}");
    }
}

fn read_key(contents: &str) -> Option<&str> {
    contents
        .trim_start_matches('\u{feff}')
        .lines()
        .find_map(|line| {
            let line = line.trim();
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (name, value) = line.split_once('=')?;
            (name.trim() == KEY).then(|| nonempty(value)).flatten()
        })
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value);
    (!value.is_empty()).then_some(value)
}
