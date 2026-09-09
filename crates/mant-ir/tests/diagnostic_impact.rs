//! Public producer coverage must survive serialization without code conventions.
use mant_ir::{Diagnostic, DiagnosticImpact, DiagnosticLevel, semantics_complete};

fn finding(impact: DiagnosticImpact, level: DiagnosticLevel, code: Option<&str>) -> Diagnostic {
    Diagnostic {
        impact,
        level,
        code: code.map(str::to_owned),
        message: "producer finding".into(),
        source: None,
    }
}

#[test]
fn coverage_is_explicit_and_independent_of_severity_or_code() {
    assert!(semantics_complete(&[]));
    for level in [
        DiagnosticLevel::Style,
        DiagnosticLevel::Warning,
        DiagnosticLevel::Error,
        DiagnosticLevel::Unsupported,
    ] {
        for code in [
            None,
            Some("third-party.new-finding"),
            Some("markdown.semantic-entry-list"),
        ] {
            let unaffected = finding(DiagnosticImpact::None, level, code);
            assert!(semantics_complete(std::slice::from_ref(&unaffected)));
            let partial = finding(DiagnosticImpact::SemanticCoverage, level, code);
            assert!(!semantics_complete(&[unaffected, partial]));
        }
    }
}

#[test]
fn coverage_round_trips_and_old_or_unknown_impact_cannot_default_to_complete() {
    for impact in [DiagnosticImpact::None, DiagnosticImpact::SemanticCoverage] {
        let original = finding(impact, DiagnosticLevel::Warning, Some("custom.coverage"));
        let mut json = serde_json::to_value(&original).unwrap();
        assert_eq!(
            serde_json::from_value::<Diagnostic>(json.clone()).unwrap(),
            original
        );
        assert!(json.get("impact").is_some());
        json.as_object_mut().unwrap().remove("impact");
        assert!(serde_json::from_value::<Diagnostic>(json.clone()).is_err());
        json["impact"] = serde_json::json!("future-impact");
        assert!(serde_json::from_value::<Diagnostic>(json).is_err());
    }
}
