use axlauncher_spi::mc_version::{JavaVersionParser, WithVersionFallback};

use super::{BuiltinJavaVersionParser, JavaVersion, JavaVersionKind, classify_java};

#[test]
fn catalog_identity_and_category_stay_separate() {
    use JavaVersionKind::*;

    for (id, manifest_type, kind) in [
        ("26.3", "release", Release),
        ("1.20.1", "release", Release),
        ("26.4-snapshot-10", "snapshot", Snapshot),
        ("26.4-snapshot-7", "snapshot", Snapshot),
        ("26.1-pre-1", "snapshot", PreRelease),
        ("1.14.2 Pre-Release 4", "snapshot", PreRelease),
        ("1.20.2-pre1", "snapshot", PreRelease),
        ("26.1-rc-1", "snapshot", ReleaseCandidate),
        ("1.21.6-rc1", "snapshot", ReleaseCandidate),
        ("20w14infinite", "snapshot", Snapshot),
        ("26w14a", "snapshot", Snapshot),
        ("3D Shareware v1.34", "snapshot", Snapshot),
        ("rd-132211", "old_alpha", OldAlpha),
        ("b1.7.3", "old_beta", OldBeta),
        ("future-snapshot-shape", "snapshot", Snapshot),
    ] {
        let parsed = classify_java(id, Some(manifest_type)).expect(id);
        assert_eq!((parsed.id(), parsed.kind()), (id, kind));
    }
    assert!(classify_java("26.3", Some("something-new")).is_none());
    assert!(classify_java("26.3", None).is_none());
}

struct NamedSnapshot;

impl JavaVersionParser for NamedSnapshot {
    fn parse<'input>(&self, _: &'input str, _: Option<&str>) -> Option<JavaVersion<'input>> {
        Some(JavaVersion::new(
            "26.4-snapshot-10",
            JavaVersionKind::Snapshot,
        ))
    }
}

#[test]
fn only_an_unrecognized_input_reaches_the_optional_parser() {
    let parser = WithVersionFallback::new(BuiltinJavaVersionParser, NamedSnapshot);
    assert_eq!(parser.parse("26.3", Some("release")).unwrap().id(), "26.3");
    assert_eq!(
        parser.parse("unnamed snapshot", None).unwrap().id(),
        "26.4-snapshot-10"
    );
}
