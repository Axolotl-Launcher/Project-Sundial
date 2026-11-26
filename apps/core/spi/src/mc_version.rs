//! Read-only version identity and an opt-in fallback for unrecognized input.

use std::borrow::Cow;

/// A recognized Java version category. It does not imply installability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaVersionKind {
    Release,
    Snapshot,
    PreRelease,
    ReleaseCandidate,
    OldBeta,
    OldAlpha,
}

/// The canonical ID and category found by a version parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaVersion<'input> {
    id: Cow<'input, str>,
    kind: JavaVersionKind,
}

impl<'input> JavaVersion<'input> {
    pub fn new(id: impl Into<Cow<'input, str>>, kind: JavaVersionKind) -> Self {
        Self {
            id: id.into(),
            kind,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn kind(&self) -> JavaVersionKind {
        self.kind
    }
}

/// Resolves a name without opening files or performing installation.
pub trait JavaVersionParser {
    /// An unrecognized input yields `None`. Only a trusted Mojang catalog
    /// entry should supply `manifest_type`.
    fn parse<'input>(
        &self,
        id: &'input str,
        manifest_type: Option<&str>,
    ) -> Option<JavaVersion<'input>>;
}

/// Consults a secondary parser without overriding a recognized primary result.
pub struct WithVersionFallback<Primary, Fallback> {
    primary: Primary,
    fallback: Fallback,
}

impl<Primary, Fallback> WithVersionFallback<Primary, Fallback> {
    pub fn new(primary: Primary, fallback: Fallback) -> Self {
        Self { primary, fallback }
    }
}

impl<Primary: JavaVersionParser, Fallback: JavaVersionParser> JavaVersionParser
    for WithVersionFallback<Primary, Fallback>
{
    fn parse<'input>(
        &self,
        id: &'input str,
        manifest_type: Option<&str>,
    ) -> Option<JavaVersion<'input>> {
        self.primary
            .parse(id, manifest_type)
            .or_else(|| self.fallback.parse(id, manifest_type))
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/spi/src/mc_version.rs"]
mod tests;
