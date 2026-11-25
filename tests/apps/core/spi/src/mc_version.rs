use std::cell::Cell;

use super::*;

struct Parser<'counter> {
    recognized: bool,
    calls: &'counter Cell<usize>,
}

impl JavaVersionParser for Parser<'_> {
    fn parse<'input>(
        &self,
        id: &'input str,
        manifest_type: Option<&str>,
    ) -> Option<JavaVersion<'input>> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(manifest_type, Some("snapshot"));
        self.recognized
            .then(|| JavaVersion::new(id, JavaVersionKind::Snapshot))
    }
}

#[test]
fn the_decorator_only_calls_fallback_when_primary_returns_none() {
    for primary_recognizes in [true, false] {
        let primary_calls = Cell::new(0);
        let fallback_calls = Cell::new(0);
        let parser = WithVersionFallback::new(
            Parser {
                recognized: primary_recognizes,
                calls: &primary_calls,
            },
            Parser {
                recognized: true,
                calls: &fallback_calls,
            },
        );
        assert_eq!(
            parser.parse("preview", Some("snapshot")).unwrap().id(),
            "preview"
        );
        assert_eq!(primary_calls.get(), 1);
        assert_eq!(fallback_calls.get(), usize::from(!primary_recognizes));
    }
}
