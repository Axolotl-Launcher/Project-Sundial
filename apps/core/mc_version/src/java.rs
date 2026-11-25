use axlauncher_spi::mc_version::{JavaVersion, JavaVersionKind, JavaVersionParser};

/// Classifies a Mojang catalog entry without changing its ID.
pub fn classify_java<'input>(
    id: &'input str,
    manifest_type: Option<&str>,
) -> Option<JavaVersion<'input>> {
    let kind = match manifest_type? {
        "release" => JavaVersionKind::Release,
        "snapshot" => split_development(id)
            .map_or(JavaVersionKind::Snapshot, |(_, development)| {
                development.kind
            }),
        "old_beta" => JavaVersionKind::OldBeta,
        "old_alpha" => JavaVersionKind::OldAlpha,
        _ => return None,
    };
    Some(JavaVersion::new(id, kind))
}

/// The same built-in classifier when a caller explicitly composes SPI parsers.
#[derive(Debug, Clone, Copy, Default)]
pub struct BuiltinJavaVersionParser;

impl JavaVersionParser for BuiltinJavaVersionParser {
    fn parse<'input>(
        &self,
        id: &'input str,
        manifest_type: Option<&str>,
    ) -> Option<JavaVersion<'input>> {
        classify_java(id, manifest_type)
    }
}

#[derive(Debug)]
pub(super) struct Development<'id> {
    kind: JavaVersionKind,
    number: &'id str,
}

impl Development<'_> {
    pub(super) fn matches(&self, selector: &str) -> bool {
        let tag = match self.kind {
            JavaVersionKind::Snapshot => "snapshot",
            JavaVersionKind::PreRelease => "pre",
            JavaVersionKind::ReleaseCandidate => "rc",
            _ => return false,
        };
        selector.split_once('-') == Some((tag, self.number))
    }
}

pub(super) fn split_development(id: &str) -> Option<(&str, Development<'_>)> {
    use JavaVersionKind::{PreRelease, ReleaseCandidate, Snapshot};

    [
        ("-snapshot-", Snapshot),
        (" Pre-Release ", PreRelease),
        ("-pre-", PreRelease),
        ("-pre", PreRelease),
        ("-release-candidate-", ReleaseCandidate),
        ("-rc-", ReleaseCandidate),
        ("-rc", ReleaseCandidate),
    ]
    .into_iter()
    .find_map(|(separator, kind)| {
        let (base, number) = id.rsplit_once(separator)?;
        (base.contains('.') && base.split('.').all(is_number) && is_number(number))
            .then_some((base, Development { kind, number }))
    })
}

pub(super) fn is_number(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/mc_version/src/java.rs"]
mod tests;
