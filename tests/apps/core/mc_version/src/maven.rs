use super::{LoaderKind, MavenArtifact, MavenTarget};

fn builds(versions: &[&str]) -> Vec<String> {
    versions
        .iter()
        .map(|version| (*version).to_owned())
        .collect()
}

#[test]
fn forge_keeps_full_coordinates_and_never_mixes_development_lines() {
    let target = MavenTarget::classify(LoaderKind::Forge, "1.7.10").unwrap();
    let coordinate = "1.7.10-10.13.4.1614-1.7.10";
    let published = target.builds(builds(&[
        "1.7.10-10.13.4.1614",
        coordinate,
        "1.7.10-10.13.4/1614",
        "1.7.10-snapshot-1-10.13.4.1614",
    ]));
    assert_eq!(published.find(coordinate).unwrap().coordinate(), coordinate);
    assert_eq!(
        published.find("10.13.4.1614").unwrap().coordinate(),
        "1.7.10-10.13.4.1614"
    );
    assert_eq!(published.into_offers().len(), 1);
}

#[test]
fn neoforge_uses_the_build_alias_only_for_its_legacy_repository() {
    let target = MavenTarget::classify(LoaderKind::NeoForge, "1.20.1").unwrap();
    assert_eq!(target.artifact(), MavenArtifact::NeoForge1201);
    let published = target.builds(builds(&["1.20.1-47.1.5", "1.20.1-47.1.5-beta", "20.2.93"]));
    assert_eq!(
        published.find("47.1.5").unwrap().coordinate(),
        "1.20.1-47.1.5"
    );
    assert_eq!(published.preferred().unwrap().coordinate(), "1.20.1-47.1.5");

    let target = MavenTarget::classify(LoaderKind::NeoForge, "1.20.2").unwrap();
    let published = target.builds(builds(&["1.20.1-47.1.5", "20.2.93"]));
    assert!(published.find("47.1.5").is_none());
}

#[test]
fn neoforge_scopes_each_selector_and_patch_to_its_exact_game() {
    let cases = [
        ("26.1", "26.1.0.1-beta"),
        ("26.1.2", "26.1.2.130"),
        ("26.1-snapshot-1", "26.1.0.0-alpha.1+snapshot-1"),
        ("26.1-pre-3", "26.1.0.0-alpha.15+pre-3"),
        ("26.1-rc-1", "26.1.0.0-alpha.16+rc-1"),
        ("25w14craftmine", "0.25w14craftmine.3-beta"),
        ("1.21", "21.0.167"),
    ];
    let versions: Vec<_> = cases.iter().map(|(_, version)| *version).collect();
    for (game, coordinate) in cases {
        let target = MavenTarget::classify(LoaderKind::NeoForge, game).unwrap();
        let published = target.builds(builds(&versions));
        assert_eq!(published.preferred().unwrap().coordinate(), coordinate);
        assert_eq!(published.into_offers().len(), 1, "{game}");
    }
    assert!(MavenTarget::classify(LoaderKind::NeoForge, "Custom 26.1").is_none());
    assert!(MavenTarget::classify(LoaderKind::Fabric, "26.1").is_none());
}
