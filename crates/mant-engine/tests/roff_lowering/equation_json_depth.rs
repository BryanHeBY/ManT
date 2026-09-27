//! Deep equation projections remain consumable through the real JSON boundary.

use libmandoc_rs::Parser;
use mant_ir::{Block, DiagnosticImpact};
use mant_loader::load_roff_bytes;
use mant_protocol::{QueryBundle, SearchCase, SearchQuery, SearchScope, SearchSyntax};

fn source(equation: &str) -> String {
    format!(".TH DEEP 7\n.SH DESCRIPTION\n.EQ\n{equation}\n.EN\n")
}

fn sqrt(depth: usize) -> String {
    format!("{}x{}", "sqrt { ".repeat(depth), " }".repeat(depth))
}

fn native_equation_text(source: &str) -> String {
    let report = Parser::default()
        .parse_bytes("deep.7", source.as_bytes())
        .expect("fixed CVS parser accepts equation");
    let mut pending = vec![&report.document.root];
    while let Some(node) = pending.pop() {
        if let Some(equation) = &node.equation {
            return equation.readable_text();
        }
        pending.extend(node.children.iter());
    }
    panic!("expected native equation node");
}

fn equation_value(document: &mant_ir::Document) -> &str {
    document
        .sections
        .iter()
        .flat_map(|section| &section.blocks)
        .find_map(|block| match block {
            Block::Equation { value, .. } => Some(value.as_str()),
            _ => None,
        })
        .expect("display equation")
}

fn search_x(content: &mant_ir::ResolvedContent) -> usize {
    mant_query::search_query(
        content,
        &SearchQuery {
            pattern: "x".into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .expect("search equation text")
    .total as usize
}

#[test]
fn equation_depth_boundary_keeps_text_and_query_bundle_json_readable() {
    // Exact inputs 23/24/32 sqrt levels were checked with the pinned
    // reference -Thtml/-Tlint before this assertion. CVS eqn.c::eqn_box_alloc
    // retains each sqrt/list level; eqn_html.c::eqn_box renders the inner x.
    for (depth, summarized) in [(23, false), (24, true), (32, true)] {
        let source = source(&sqrt(depth));
        let native = native_equation_text(&source);
        assert!(native.contains('x'));
        let content = load_roff_bytes(source.as_bytes()).expect("lower source");
        let document = content.document.as_ref().unwrap();
        assert_eq!(equation_value(document), native, "depth {depth}");
        assert_eq!(search_x(&content), 1, "depth {depth}");
        assert_eq!(
            document.diagnostics.iter().any(|finding| {
                finding.code.as_deref() == Some("manual.equation-structure-depth-summarized")
                    && finding.impact == DiagnosticImpact::SemanticCoverage
            }),
            summarized,
            "depth {depth}"
        );
        assert!(mant_ir::content_complete(&document.diagnostics));

        let bundle = QueryBundle::from(&content);
        let encoded = serde_json::to_string(&bundle).expect("serialize full query");
        let decoded: QueryBundle =
            serde_json::from_str(&encoded).expect("deserialize actual JSON text");
        assert_eq!(bundle, decoded, "depth {depth}");
        let decoded_content: mant_ir::ResolvedContent = decoded.into();
        assert_eq!(search_x(&decoded_content), 1, "decoded depth {depth}");
    }
}

#[test]
fn summarized_nested_operators_keep_parent_grouping_and_operands() {
    // Both exact 50-group inputs were checked with fixed CVS -Thtml/-Tlint:
    // eqn.c::eqn_box_makebinary attaches the outer operator;
    // eqn_html.c::eqn_box nests mfrac/msup nodes and retains a, b, c.
    for (operator, expected) in [("over", "(a / b) / c"), ("sup", "(a ^ b) ^ c")] {
        let equation = format!(
            "{}a {operator} b{} {operator} c",
            "{ ".repeat(50),
            " }".repeat(50)
        );
        let source = source(&equation);
        let native = native_equation_text(&source);
        assert_eq!(native, expected);
        let content = load_roff_bytes(source.as_bytes()).expect("lower nested operator");
        let document = content.document.as_ref().unwrap();
        assert_eq!(equation_value(document), native);
        assert!(document.diagnostics.iter().any(|finding| {
            finding.code.as_deref() == Some("manual.equation-structure-depth-summarized")
                && finding.impact == DiagnosticImpact::SemanticCoverage
        }));
        assert!(mant_ir::content_complete(&document.diagnostics));
        let encoded = serde_json::to_string(&QueryBundle::from(&content)).unwrap();
        let decoded: QueryBundle = serde_json::from_str(&encoded).unwrap();
        let document = decoded.document.as_ref().unwrap();
        assert!(
            document.sections[0].blocks.iter().any(|block| {
                matches!(block, Block::Equation { value, .. } if value == expected)
            })
        );
        for operand in ["a", "b", "c"] {
            assert!(document.sections[0].blocks.iter().any(|block| {
                matches!(block, Block::Equation { value, .. } if value.contains(operand))
            }));
        }
    }
}

#[test]
fn externally_supplied_excessive_equation_json_still_hits_recursion_guard() {
    // The 32-sqrt source was checked with fixed CVS -Thtml/-Tlint above.
    // This test changes only the incoming wire tree, preserving serde_json's
    // default rejection of deeper untrusted JSON after producer-side folding.
    let content = load_roff_bytes(source(&sqrt(32)).as_bytes()).unwrap();
    let mut value = serde_json::to_value(QueryBundle::from(&content)).unwrap();
    let mut expression = serde_json::json!({
        "kind": "Text", "font": "None", "position": "None",
        "actualArgs": 0, "text": "x", "children": []
    });
    for _ in 0..70 {
        expression = serde_json::json!({
            "kind": "List", "font": "None", "position": "None",
            "actualArgs": 1, "children": [expression]
        });
    }
    value["document"]["sections"][0]["blocks"][0]["expression"] = expression;
    let hostile = serde_json::to_string(&value).unwrap();
    let error = serde_json::from_str::<QueryBundle>(&hostile).unwrap_err();
    assert!(error.to_string().contains("recursion limit exceeded"));
}
