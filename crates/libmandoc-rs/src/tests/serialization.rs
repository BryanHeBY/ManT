//! Feature-gated public report and diagnostic wire shapes.

use super::*;

#[cfg(feature = "serde")]
fn find_serialized_equation(value: &serde_json::Value) -> Option<&serde_json::Value> {
    match value {
        serde_json::Value::Array(values) => values.iter().find_map(find_serialized_equation),
        serde_json::Value::Object(fields) => fields
            .get("equation")
            .filter(|equation| !equation.is_null())
            .or_else(|| fields.values().find_map(find_serialized_equation)),
        _ => None,
    }
}

#[cfg(feature = "serde")]
#[test]
fn serde_feature_round_trips_the_public_parse_report() {
    let report = Parser::default()
        .parse_bytes("serde.1", b".TH SERDE 1\n.SH NAME\nserde \\- fixture\n")
        .expect("parse source for serialization");
    let encoded = serde_json::to_string(&report).expect("serialize parse report");
    let decoded: crate::ParseReport =
        serde_json::from_str(&encoded).expect("deserialize parse report");

    assert_eq!(decoded, report);
}

#[cfg(feature = "serde")]
#[test]
fn serde_feature_round_trips_the_typed_equation_tree() {
    // This exact fixture was checked with the fixed-CVS `-Ttree` and `-Tascii`
    // reference before its typed equation assertions were introduced.
    let report = Parser::default()
        .parse_bytes(
            "equation-structure.1",
            include_bytes!("../../tests/fixtures/execution/equation-structure.1"),
        )
        .expect("parse equation source for serialization");
    let encoded = serde_json::to_value(&report).expect("serialize typed equation report");
    let equation =
        find_serialized_equation(&encoded).expect("typed equation shape must be present");
    assert!(equation.get("root").is_some(), "typed equation root");

    let decoded: crate::ParseReport =
        serde_json::from_value(encoded).expect("deserialize typed equation report");
    assert_eq!(decoded, report);
}

#[cfg(feature = "serde")]
#[test]
fn serde_diagnostics_keep_the_patch_compatible_field_shape() {
    let diagnostic = Diagnostic {
        level: DiagnosticLevel::Warning,
        message: crate::diagnostics::SYNTAX_TREE_DEPTH_MESSAGE.to_owned(),
        location: None,
    };
    let encoded = serde_json::to_value(&diagnostic).expect("serialize diagnostic");

    assert_eq!(
        diagnostic.code(),
        Some(DiagnosticCode::SyntaxTreeDepthLimit)
    );
    assert!(encoded.get("code").is_none());
    assert_eq!(
        serde_json::from_value::<Diagnostic>(encoded).expect("deserialize diagnostic"),
        diagnostic
    );
}
