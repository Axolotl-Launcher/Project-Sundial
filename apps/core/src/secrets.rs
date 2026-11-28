//! Resolves launcher secrets once, before any consumer can use them.

use std::env;
use std::path::Path;
use std::sync::LazyLock;

use axlauncher_network_spi::auth::ApiKey;
use axlauncher_spi::curseforge::CatalogError;

const KEY_NAME: &str = "CURSEFORGE_API_KEY";
#[cfg(not(debug_assertions))]
const BUILD_DEFAULT: Option<&str> = option_env!("CURSEFORGE_API_KEY");
#[cfg(debug_assertions)]
const BUILD_DEFAULT: Option<&str> = None;
const RUNTIME_FILES: [&str; 4] = if cfg!(debug_assertions) {
    [
        ".env.development.local",
        ".env.local",
        ".env.development",
        ".env",
    ]
} else {
    [
        ".env.production.local",
        ".env.local",
        ".env.production",
        ".env",
    ]
};

static CURSEFORGE: LazyLock<Result<ApiKey, CatalogError>> = LazyLock::new(load_curseforge);

pub(crate) fn curseforge() -> Result<&'static ApiKey, CatalogError> {
    CURSEFORGE.as_ref().map_err(|error| *error)
}

fn load_curseforge() -> Result<ApiKey, CatalogError> {
    resolve(process_key()?.as_deref(), Path::new("."), BUILD_DEFAULT)
}

fn process_key() -> Result<Option<String>, CatalogError> {
    match env::var(KEY_NAME) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(CatalogError::InvalidApiKey),
    }
}

fn resolve(
    process: Option<&str>,
    directory: &Path,
    build: Option<&str>,
) -> Result<ApiKey, CatalogError> {
    if let Some(key) = validate(process)? {
        return Ok(key);
    }
    for manifest in RUNTIME_FILES {
        let Ok(contents) = std::fs::read_to_string(directory.join(manifest)) else {
            continue;
        };
        if let Some(key) = validate(file_value(&contents)?)? {
            return Ok(key);
        }
    }
    validate(build)?.ok_or(CatalogError::MissingApiKey)
}

fn validate(value: Option<&str>) -> Result<Option<ApiKey>, CatalogError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    ApiKey::new(value)
        .ok_or(CatalogError::InvalidApiKey)
        .map(Some)
}

fn file_value(contents: &str) -> Result<Option<&str>, CatalogError> {
    contents
        .trim_start_matches('\u{feff}')
        .lines()
        .rev()
        .find_map(|line| {
            let line = line.trim();
            let (name, value) = line
                .strip_prefix("export ")
                .unwrap_or(line)
                .split_once('=')?;
            (name.trim() == KEY_NAME).then_some(value)
        })
        .map(parse_value)
        .transpose()
}

fn parse_value(value: &str) -> Result<&str, CatalogError> {
    let value = value.trim();
    if let Some(quote) = value
        .chars()
        .next()
        .filter(|symbol| matches!(symbol, '\'' | '"'))
    {
        let (value, suffix) = value[quote.len_utf8()..]
            .split_once(quote)
            .ok_or(CatalogError::InvalidApiKey)?;
        if !suffix.trim().is_empty() && !suffix.trim_start().starts_with('#') {
            return Err(CatalogError::InvalidApiKey);
        }
        return Ok(value);
    }
    Ok(value.split_once('#').map_or(value, |(value, _)| value))
}

#[cfg(test)]
#[path = "../../../tests/apps/core/src/secrets.rs"]
mod tests;
