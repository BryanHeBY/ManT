//! Feature-gated public report and diagnostic wire shapes.

use super::*;

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
