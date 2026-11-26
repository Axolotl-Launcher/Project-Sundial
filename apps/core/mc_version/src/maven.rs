use axlauncher_spi::{LoaderKind, LoaderVersion};

mod forge;
mod neoforge;
mod order;

/// The Maven artifact that publishes a classified loader line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MavenArtifact {
    Forge,
    NeoForge,
    NeoForge1201,
}

/// The build convention selected before inspecting a loader's Maven listing.
#[derive(Debug)]
pub struct MavenTarget<'game> {
    game: &'game str,
    convention: Convention<'game>,
}

#[derive(Debug)]
enum Convention<'game> {
    Forge(forge::Target),
    NeoForge(neoforge::Target<'game>),
}

impl<'game> MavenTarget<'game> {
    /// Chooses the parser for a specific game and Maven-backed loader.
    pub fn classify(loader: LoaderKind, game: &'game str) -> Option<Self> {
        let convention = match loader {
            LoaderKind::Forge => Convention::Forge(forge::Target::new(game)),
            LoaderKind::NeoForge => Convention::NeoForge(neoforge::Target::classify(game)?),
            _ => return None,
        };
        Some(Self { game, convention })
    }

    pub fn game(&self) -> &'game str {
        self.game
    }

    pub fn loader(&self) -> LoaderKind {
        match self.convention {
            Convention::Forge(_) => LoaderKind::Forge,
            Convention::NeoForge(_) => LoaderKind::NeoForge,
        }
    }

    pub fn artifact(&self) -> MavenArtifact {
        match &self.convention {
            Convention::Forge(_) => MavenArtifact::Forge,
            Convention::NeoForge(target) => target.artifact(),
        }
    }

    /// A named Forge build's primary coordinate and, for historical builds,
    /// the equivalent spelling to try if its installer is not published.
    pub fn forge_coordinates(&self, requested: &str) -> Option<(String, Option<String>)> {
        let Convention::Forge(target) = &self.convention else {
            return None;
        };
        if !is_coordinate_version(self.game) || !is_coordinate_version(requested) {
            return None;
        }
        target.coordinates(requested)
    }

    /// Filters by game version and orders the published builds together with
    /// the exact artifact versions needed to download them. For two Forge
    /// coordinates for the same build, the repeated-game form comes first.
    pub fn builds(&self, versions: Vec<String>) -> MavenBuilds {
        let mut builds: Vec<_> = versions
            .into_iter()
            .filter_map(|coordinate| self.parse(coordinate))
            .collect();
        builds.sort_unstable_by(|left, right| {
            order::compare(&right.offer.version, &left.offer.version)
                .then_with(|| left.offer.version.cmp(&right.offer.version))
                .then_with(|| right.coordinate.cmp(&left.coordinate))
        });
        MavenBuilds(builds)
    }

    fn parse(&self, coordinate: String) -> Option<MavenBuild> {
        if !is_coordinate_version(&coordinate) {
            return None;
        }
        let details = match &self.convention {
            Convention::Forge(target) => target.parse(&coordinate),
            Convention::NeoForge(target) => target.parse(&coordinate),
        }?;
        let offer = LoaderVersion {
            version: details.version.to_owned(),
            stable: details.stable,
        };
        let alias = details.alias.map(str::to_owned);
        Some(MavenBuild {
            coordinate,
            offer,
            alias,
        })
    }
}

struct BuildDetails<'version> {
    version: &'version str,
    stable: Option<bool>,
    alias: Option<&'version str>,
}

fn is_coordinate_version(coordinate: &str) -> bool {
    !coordinate.is_empty()
        && coordinate.as_bytes().last() != Some(&b'.')
        && coordinate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".+-_".contains(&byte))
}

/// Matching published builds, ordered newest first.
#[derive(Debug)]
pub struct MavenBuilds(Vec<MavenBuild>);

impl MavenBuilds {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Chooses the newest stable build when known, otherwise the newest build.
    pub fn preferred(&self) -> Option<&MavenBuild> {
        self.0
            .iter()
            .find(|build| build.offer.stable == Some(true))
            .or_else(|| self.0.first())
    }

    /// Accepts a published coordinate, a listed build, or a known ecosystem alias.
    pub fn find(&self, requested: &str) -> Option<&MavenBuild> {
        if let Some(exact) = self.0.iter().find(|build| build.coordinate == requested) {
            return Some(exact);
        }
        self.0.iter().find(|build| {
            build.offer.version == requested || build.alias.as_deref() == Some(requested)
        })
    }

    /// Lists each selectable build once without discarding exact coordinates
    /// from the underlying resolution API.
    pub fn into_offers(self) -> Vec<LoaderVersion> {
        let mut offers: Vec<_> = self.0.into_iter().map(|build| build.offer).collect();
        offers.dedup_by(|left, right| left.version == right.version);
        offers
    }
}

/// One selectable build and the Maven version that actually publishes it.
#[derive(Debug)]
pub struct MavenBuild {
    coordinate: String,
    offer: LoaderVersion,
    alias: Option<String>,
}

impl MavenBuild {
    pub fn coordinate(&self) -> &str {
        &self.coordinate
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/mc_version/src/maven.rs"]
mod tests;
