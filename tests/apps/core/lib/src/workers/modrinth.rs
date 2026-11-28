use serde_json::json;

use super::*;

#[test]
fn validates_search_and_project_keys_before_dispatch() {
    assert!(Search::new("", 0, 100).is_ok());
    assert!(Search::new("雪 + terrain", 20, 10).is_ok());
    assert!(Search::new("", 0, 0).is_ok());
    assert!(Search::new("", 0, 101).is_err());
    assert!(Search::new("x".repeat(513), 0, 1).is_err());
    assert!(Search::new("in\nvalid", 0, 1).is_err());
    for valid in ["AABBCCDD", "my_project-2.0"] {
        assert!(ProjectKey::new(valid).is_ok());
    }
    for invalid in [
        "",
        ".",
        "..",
        "../other",
        "foo%2Fbar",
        "https://example.invalid",
        "a?b",
    ] {
        assert!(ProjectKey::new(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn validates_hashes_batches_filters_and_artifact_coordinates() {
    let sample = "ABCDEF0123456789ABCDEF0123456789ABCDEF01";
    let valid = HashLookup {
        algorithm: HashAlgorithm::Sha1,
        hash: sample.into(),
        version: None,
    };
    assert!(Operation::FileVersion(valid.clone()).validate().is_ok());
    let wrong_algorithm = HashLookup {
        algorithm: HashAlgorithm::Sha512,
        ..valid
    };
    assert_eq!(
        Operation::FileDownload(wrong_algorithm).validate(),
        Err(CatalogError::InvalidRequest)
    );
    assert!(Batch::new(vec!["id"; 101]).is_err());
    assert!(VersionId::new("../id").is_err());
    assert!(CountryCode::new("US/UK").is_err());
    let bad_coordinate = MavenCoordinate {
        project: ProjectKey::new("demo").unwrap(),
        version: VersionKey::new("1.0.0").unwrap(),
        filename: "../some.jar".into(),
    };
    assert_eq!(
        Operation::MavenArtifact(bad_coordinate).validate(),
        Err(CatalogError::InvalidRequest)
    );
    let mut search = Search::new("demo", 20, 10).unwrap();
    search.facets = vec![vec![Facet {
        field: FacetField::Downloads,
        comparison: Comparison::GreaterOrEqual,
        value: "many".into(),
    }]];
    assert_eq!(
        Operation::Search(search).validate(),
        Err(CatalogError::InvalidRequest)
    );
}

#[test]
fn v1_wire_accepts_only_public_catalog_operations() {
    for command in [
        json!({"V2": {"Project": "sodium"}}),
        json!({"V1": {"User": null}}),
        json!({"V1": {"Get": "https://example.invalid"}}),
        json!({"V1": {"Search": {"query": "", "offset": 0, "limit": 10, "token": "ignored"}}}),
    ] {
        assert!(serde_json::from_value::<Command>(command).is_err());
    }
    let Command::V1(decoded) =
        serde_json::from_value(json!({"V1": {"Search": {"query": "", "offset": 0, "limit": 101}}}))
            .unwrap();
    assert!(matches!(decoded, Operation::Search(_)));
    assert_eq!(decoded.validate(), Err(CatalogError::InvalidRequest));
}
