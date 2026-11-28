use serde_json::json;

use super::{CatalogError, Command, Operation, ProjectKey, Search};

#[test]
fn validates_search_and_project_keys_before_dispatch() {
    assert!(Search::new("", 0, 100).is_ok());
    assert!(Search::new("雪 + terrain", 20, 10).is_ok());
    assert!(Search::new("", 0, 0).is_err());
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
        serde_json::from_value(json!({"V1": {"Search": {"query": "", "offset": 0, "limit": 0}}}))
            .unwrap();
    assert!(matches!(decoded, Operation::Search(_)));
    assert_eq!(decoded.validate(), Err(CatalogError::InvalidRequest));
}
