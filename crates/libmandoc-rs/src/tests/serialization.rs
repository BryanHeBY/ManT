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

#[test]
fn serde_retains_ip_label_provenance_and_defaults_older_snapshots() {
    // The exact direct-register control is recorded in the engine's pinned
    // IP matrix. blk_imp() attaches the receipt to the executed label.
    let report = Parser::default()
        .parse_bytes(
            "probe.1",
            b".TH PROBE 1 \"2026-01-01\"\n.SH DESCRIPTION\n.nr XX 0 1\n\
              .IP \\n+[XX]\nfirst\n.IP \\n+[XX]\nsecond\n",
        )
        .unwrap();
    let label = find_node(&report.document.root, &|node| {
        node.flags.ip_incremented_register
    })
    .unwrap();
    let mut flags = serde_json::to_value(label.flags).unwrap();
    assert_eq!(flags["ip_incremented_register"], true);
    assert_eq!(
        serde_json::from_value::<crate::NodeFlags>(flags.clone()).unwrap(),
        label.flags
    );
    flags
        .as_object_mut()
        .unwrap()
        .remove("ip_incremented_register");
    let older = serde_json::from_value::<crate::NodeFlags>(flags.clone()).unwrap();
    assert!(!older.ip_incremented_register);
    assert_eq!(serde_json::to_value(older).unwrap(), flags);
}

#[cfg(feature = "serde")]
#[test]
fn serde_diagnostics_preserve_the_structured_coverage_reason() {
    let diagnostic = Diagnostic {
        code: Some(DiagnosticCode::SyntaxTreeDepthLimit),
        level: DiagnosticLevel::Warning,
        message: crate::diagnostics::SYNTAX_TREE_DEPTH_MESSAGE.to_owned(),
        location: None,
    };
    let encoded = serde_json::to_value(&diagnostic).expect("serialize diagnostic");

    assert_eq!(
        diagnostic.code(),
        Some(DiagnosticCode::SyntaxTreeDepthLimit)
    );
    assert_eq!(encoded["code"], "SyntaxTreeDepthLimit");
    assert_eq!(
        serde_json::from_value::<Diagnostic>(encoded).expect("deserialize diagnostic"),
        diagnostic
    );
}
