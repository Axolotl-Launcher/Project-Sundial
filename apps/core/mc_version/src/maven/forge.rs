use crate::java::is_number;

use super::BuildDetails;

const HISTORICAL_BUILD_COMPONENTS: usize = 4;

#[derive(Debug)]
pub(super) struct Target {
    prefix: String,
    repeated_game: String,
}

impl Target {
    pub(super) fn new(game: &str) -> Self {
        Self {
            prefix: format!("{game}-"),
            repeated_game: format!("-{game}"),
        }
    }

    pub(super) fn coordinates(&self, requested: &str) -> Option<(String, Option<String>)> {
        let primary = if requested.starts_with(&self.prefix) {
            requested.to_owned()
        } else {
            format!("{}{requested}", self.prefix)
        };
        let build = self.parse(&primary)?.version;
        let alternate = self.alternate(&primary, build);
        Some((primary, alternate))
    }

    fn alternate(&self, coordinate: &str, build: &str) -> Option<String> {
        let mut parts = build.split('.');
        if parts.clone().count() != HISTORICAL_BUILD_COMPONENTS || !parts.all(is_number) {
            return None;
        }
        Some(if coordinate.ends_with(&self.repeated_game) {
            format!("{}{build}", self.prefix)
        } else {
            format!("{coordinate}{}", self.repeated_game)
        })
    }

    pub(super) fn parse<'version>(
        &self,
        coordinate: &'version str,
    ) -> Option<BuildDetails<'version>> {
        let build = coordinate.strip_prefix(&self.prefix)?;
        if !build.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return None;
        }
        Some(BuildDetails {
            version: build.strip_suffix(&self.repeated_game).unwrap_or(build),
            stable: None,
            alias: None,
        })
    }
}
