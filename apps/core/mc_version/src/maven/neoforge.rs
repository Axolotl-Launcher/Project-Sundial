use crate::java::{Development, is_number, split_development};

use super::{BuildDetails, MavenArtifact};

const FORGE_GAME: &str = "1.20.1";
const FIRST_CALENDAR_YEAR: u8 = 26;

/// The game portion encoded in a particular NeoForge Maven series.
#[derive(Debug)]
pub(super) enum Target<'game> {
    Forge,
    Numbered {
        series: String,
        development: Option<Development<'game>>,
    },
}

impl<'game> Target<'game> {
    pub(super) fn classify(game: &'game str) -> Option<Self> {
        if game == FORGE_GAME {
            return Some(Self::Forge);
        }
        if is_weekly_id(game) {
            return Some(Self::Numbered {
                series: format!("0.{game}"),
                development: None,
            });
        }
        let (base, development) = split_development(game)
            .map_or((game, None), |(base, development)| {
                (base, Some(development))
            });
        Some(Self::Numbered {
            series: numbered_series(base)?,
            development,
        })
    }

    pub(super) fn artifact(&self) -> MavenArtifact {
        match self {
            Self::Forge => MavenArtifact::NeoForge1201,
            Self::Numbered { .. } => MavenArtifact::NeoForge,
        }
    }

    pub(super) fn parse<'version>(
        &self,
        coordinate: &'version str,
    ) -> Option<BuildDetails<'version>> {
        match self {
            Self::Forge => forge_build(coordinate),
            Self::Numbered {
                series,
                development,
            } => numbered_build(coordinate, series, development.as_ref()),
        }
    }
}

fn forge_build(coordinate: &str) -> Option<BuildDetails<'_>> {
    let alias = coordinate.strip_prefix(FORGE_GAME)?.strip_prefix('-')?;
    let (build, qualifier) = alias
        .split_once('-')
        .map_or((alias, None), |(build, suffix)| (build, Some(suffix)));
    (!alias.contains('+') && build.split('.').all(is_number)).then_some(BuildDetails {
        version: coordinate,
        stable: Some(qualifier.is_none()),
        alias: Some(alias),
    })
}

fn numbered_build<'version>(
    coordinate: &'version str,
    series: &str,
    development: Option<&Development<'_>>,
) -> Option<BuildDetails<'version>> {
    let (build, selector) = coordinate
        .split_once('+')
        .map_or((coordinate, None), |(build, tag)| (build, Some(tag)));
    let (number, qualifier) = build
        .split_once('-')
        .map_or((build, None), |(number, suffix)| (number, Some(suffix)));
    let (coordinate_series, counter) = number.rsplit_once('.')?;
    (coordinate_series == series
        && is_number(counter)
        && matches_development(development, selector))
    .then_some(BuildDetails {
        version: coordinate,
        stable: Some(qualifier.is_none() && selector.is_none()),
        alias: None,
    })
}

fn numbered_series(game: &str) -> Option<String> {
    let mut parts = game.split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    let patch = parts.next().unwrap_or("0");
    if !is_number(minor) || !is_number(patch) || parts.next().is_some() {
        return None;
    }
    if major == "1" {
        return Some(format!("{minor}.{patch}"));
    }
    (major.parse::<u8>().ok()? >= FIRST_CALENDAR_YEAR).then(|| format!("{major}.{minor}.{patch}"))
}

fn matches_development(development: Option<&Development<'_>>, selector: Option<&str>) -> bool {
    match (development, selector) {
        (None, None) => true,
        (Some(development), Some(selector)) => development.matches(selector),
        _ => false,
    }
}

fn is_weekly_id(id: &str) -> bool {
    matches!(
        id.as_bytes(),
        [
            b'0'..=b'9',
            b'0'..=b'9',
            b'w',
            b'0'..=b'9',
            b'0'..=b'9',
            _,
            ..
        ]
    ) && id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
