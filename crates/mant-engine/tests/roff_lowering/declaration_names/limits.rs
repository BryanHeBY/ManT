//! Explicit native selector limits retain readable forms and report coverage.

use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LimitCase {
    id: String,
    source: String,
    declared_count: usize,
    expected_names: Vec<String>,
    native_head: String,
    name_limit: bool,
}

fn limits() -> Vec<LimitCase> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!("limits.json")).unwrap();
    assert_eq!(fixture["header"]["count"], 3);
    serde_json::from_value(fixture["cases"].clone()).unwrap()
}

fn explicit_head_flags(node: &libmandoc_rs::Node) -> Vec<String> {
    fn visit(node: &libmandoc_rs::Node, output: &mut Vec<String>) {
        if node.kind == libmandoc_rs::NodeKind::Element && node.macro_token.as_deref() == Some("Fl")
        {
            let [operand] = node.children.as_slice() else {
                panic!("fixture Fl has exactly one actual TEXT operand");
            };
            assert_eq!(operand.kind, libmandoc_rs::NodeKind::Text);
            output.push(format!("-{}", operand.text.as_deref().unwrap()));
        }
        for child in &node.children {
            visit(child, output);
        }
    }
    if node.kind == libmandoc_rs::NodeKind::Head && node.macro_token.as_deref() == Some("It") {
        let mut flags = Vec::new();
        visit(node, &mut flags);
        return flags;
    }
    for child in &node.children {
        let flags = explicit_head_flags(child);
        if !flags.is_empty() {
            return flags;
        }
    }
    Vec::new()
}

#[test]
fn native_declared_name_limits_cross_json_queries_without_losing_forms() {
    // Exact multiline Xo sources ran all five pristine profiles first.
    // termp_fl_pre emits each dash then its TEXT child; no native parser
    // limit fired. The 256-selector cap belongs to ManT's documented grammar.
    for case in limits() {
        let native = libmandoc_rs::Parser::default()
            .parse_bytes("limits.1", case.source.as_bytes())
            .unwrap();
        assert_eq!(native.diagnostics.len(), 0);
        let native_flags = explicit_head_flags(&native.document.root);
        assert_eq!(
            native_flags.len(),
            case.declared_count,
            "{}: actual HEAD",
            case.id
        );
        assert_eq!(native_flags.join(" "), case.native_head);
        let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&original)).unwrap();
        let wire: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let content: ResolvedContent = wire.into();
        assert_eq!(content, original);
        let document = content.document.as_ref().unwrap();
        let entries = owners(document);
        let [owner] = entries.as_slice() else {
            panic!("one original declaration owner")
        };
        let facts = owner.facts().unwrap();
        assert_eq!(facts.names, case.expected_names);
        let forms = owner
            .forms()
            .unwrap()
            .iter()
            .map(mant_ir::inline_plain_text)
            .collect::<Vec<_>>();
        assert_eq!(flow_words(&forms), case.native_head);
        let losses = document
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code.as_deref() == Some("manual.semantic-entry.name-limit")
            })
            .collect::<Vec<_>>();
        assert_eq!(losses.len(), usize::from(case.name_limit));
        for loss in losses {
            assert_eq!(loss.impact, mant_ir::DiagnosticImpact::SemanticCoverage);
            assert_eq!(loss.source, owner.source());
        }
        assert!(mant_ir::content_complete(&document.diagnostics));
        assert_eq!(
            mant_ir::semantics_complete(&document.diagnostics),
            !case.name_limit
        );
        let outline = mant_query::build_outline(&content).unwrap();
        assert!(outline.content_complete);
        assert_eq!(outline.semantics_complete, !case.name_limit);
        let encoded = serde_json::to_string(&outline).unwrap();
        let decoded: mant_protocol::QueryOutline = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, outline);
        assert_eq!(search(&content, "BodyWord").total, 1);
        let read = mant_loader::load_markdown_text(&render_markdown(&content), None).unwrap();
        assert!(
            flow_words(&text_blocks(read.document.as_ref().unwrap())).contains(&case.native_head)
        );
    }
}
