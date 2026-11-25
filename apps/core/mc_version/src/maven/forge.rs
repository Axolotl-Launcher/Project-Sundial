use super::BuildDetails;

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
