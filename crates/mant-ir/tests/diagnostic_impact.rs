//! Public producer coverage must survive serialization without code conventions.
use mant_ir::{
    CoverageScope, Diagnostic, DiagnosticImpact, DiagnosticLevel, SourceKey, semantics_complete,
};
use serde_json::json;
use std::num::NonZeroU32;

fn finding(impact: DiagnosticImpact, level: DiagnosticLevel, code: Option<&str>) -> Diagnostic {
    Diagnostic {
        impact,
        level,
        code: code.map(str::to_owned),
        message: "producer finding".into(),
        source: None,
        source_key: None,
        coverage_scope: None,
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

#[test]
fn optional_tagged_coverage_scope_round_trips_all_five_domains() {
    let key = NonZeroU32::new(7).unwrap();
    for (scope, wire) in [
        (CoverageScope::Document, json!({"kind":"document"})),
        (
            CoverageScope::Section { key },
            json!({"kind":"section","key":7}),
        ),
        (
            CoverageScope::Owner { key },
            json!({"kind":"owner","key":7}),
        ),
        (
            CoverageScope::Region { key },
            json!({"kind":"region","key":7}),
        ),
        (
            CoverageScope::Source {
                key: SourceKey::FIRST,
            },
            json!({"kind":"source","key":1}),
        ),
    ] {
        let mut diagnostic = finding(
            DiagnosticImpact::SemanticCoverage,
            DiagnosticLevel::Unsupported,
            Some("annotated.coverage.link.unverified"),
        );
        diagnostic.coverage_scope = Some(scope);
        let value = serde_json::to_value(&diagnostic).unwrap();
        assert_eq!(value["coverageScope"], wire);
        assert_eq!(
            serde_json::from_value::<Diagnostic>(value).unwrap(),
            diagnostic
        );
    }
    assert!(
        serde_json::to_value(finding(
            DiagnosticImpact::None,
            DiagnosticLevel::Style,
            None
        ))
        .unwrap()
        .get("coverageScope")
        .is_none()
    );
}

#[test]
fn coverage_scope_wire_rejects_unknown_or_ambiguous_shapes() {
    let base = serde_json::to_value(finding(
        DiagnosticImpact::SemanticCoverage,
        DiagnosticLevel::Unsupported,
        None,
    ))
    .unwrap();
    for scope in [
        json!({"kind":"unknown"}),
        json!({"kind":"owner"}),
        json!({"kind":"owner","key":0}),
        json!({"kind":"source","key":0}),
        json!({"kind":"document","key":1}),
        json!({"kind":"owner","key":1,"source":1}),
        json!({"kind":"owner","key":-1}),
    ] {
        let mut value = base.clone();
        value["coverageScope"] = scope.clone();
        assert!(
            serde_json::from_value::<Diagnostic>(value).is_err(),
            "accepted {scope}"
        );
    }
}
