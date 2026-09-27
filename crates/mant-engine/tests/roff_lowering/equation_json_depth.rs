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

fn nested_mdoc_source(list_depth: usize, equation: &str) -> String {
    format!(
        ".Dd September 28, 2026\n.Dt REVIEW 7\n.Os\n.Sh DESCRIPTION\n{}.EQ\n{equation}\n.EN\n{}",
        ".Bl -tag -width key\n.It key\n".repeat(list_depth),
        ".El\n".repeat(list_depth)
    )
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

fn nested_equation_value(blocks: &[Block]) -> Option<&str> {
    for block in blocks {
        match block {
            Block::Equation { value, .. } => return Some(value),
            Block::List { items, .. } => {
                for item in items {
                    if let Some(value) = nested_equation_value(&item.blocks) {
                        return Some(value);
                    }
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    if let Some(value) = nested_equation_value(&item.description) {
                        return Some(value);
                    }
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        if let Some(value) = nested_equation_value(&cell.blocks) {
                            return Some(value);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
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
    for (depth, summarized) in [(23, false), (24, false), (32, true)] {
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
fn surrounding_document_depth_reduces_equation_wire_budget() {
    // These exact 0/6/12-list, 1/23/24/32-sqrt inputs were run with the
    // pinned -Thtml/-Tlint oracle before this assertion. mdoc_macro.c::blk_full
    // nests each Bl/It body; mdoc_html.c::mdoc_bl_pre/mdoc_it_pre renders each
    // definition list; eqn.c::eqn_box_alloc retains the sqrt/list boxes.
    for (lists, roots, summarized) in [
        (0, 24, false),
        (0, 32, true),
        (6, 1, false),
        (6, 23, true),
        (6, 24, true),
        (12, 24, true),
    ] {
        let source = nested_mdoc_source(lists, &sqrt(roots));
        let native = native_equation_text(&source);
        let content = load_roff_bytes(source.as_bytes()).unwrap();
        let document = content.document.as_ref().unwrap();
        assert_eq!(
            document.sections[0]
                .blocks
                .iter()
                .find_map(|block| nested_equation_value(std::slice::from_ref(block))),
            Some(native.as_str()),
            "lists={lists}, roots={roots}"
        );
        assert_eq!(search_x(&content), 1, "lists={lists}, roots={roots}");
        assert_eq!(
            document.diagnostics.iter().any(|finding| {
                finding.code.as_deref() == Some("manual.equation-structure-depth-summarized")
                    && finding.impact == DiagnosticImpact::SemanticCoverage
            }),
            summarized,
            "lists={lists}, roots={roots}"
        );
        assert!(mant_ir::content_complete(&document.diagnostics));
        let bundle = QueryBundle::from(&content);
        let encoded = serde_json::to_string(&bundle).unwrap();
        let decoded: QueryBundle = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, bundle);
        assert_eq!(
            search_x(&mant_ir::ResolvedContent::from(decoded)),
            1,
            "decoded lists={lists}, roots={roots}"
        );
    }
}

#[test]
fn document_nesting_alone_cannot_emit_unreadable_query_json() {
    // This exact 36-list source was checked with fixed CVS -Thtml/-Tlint:
    // mdoc_macro.c::blk_full retains every Bl/It scope and
    // mdoc_html.c::mdoc_bl_pre/mdoc_it_pre renders the enclosed eqn x.
    // The document's JSON path exceeds the reader budget even for a one-box
    // equation, so the deepest structural subtree keeps its readable text.
    let source = nested_mdoc_source(36, "x");
    let content = load_roff_bytes(source.as_bytes()).unwrap();
    let document = content.document.as_ref().unwrap();
    assert_eq!(search_x(&content), 1);
    assert!(document.diagnostics.iter().any(|finding| {
        finding.code.as_deref() == Some("manual.document-structure-depth-summarized")
            && finding.impact == DiagnosticImpact::SemanticCoverage
    }));
    assert!(mant_ir::content_complete(&document.diagnostics));
    assert!(!mant_ir::semantics_complete(&document.diagnostics));
    let bundle = QueryBundle::from(&content);
    let encoded = serde_json::to_string(&bundle).unwrap();
    let decoded: QueryBundle = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, bundle);
    assert_eq!(search_x(&mant_ir::ResolvedContent::from(decoded)), 1);
}

#[test]
fn deep_document_without_equation_keeps_body_and_wire_contract() {
    // This exact 36-list source with ordinary prose was checked with fixed
    // CVS -Thtml/-Tlint before the assertion. mdoc_macro.c::blk_full keeps
    // the nested It bodies and mdoc_html.c::mdoc_it_pre prints the innermost
    // `safe tail` text even when no eqn node occurs.
    let source = format!(
        ".Dd September 28, 2026\n.Dt REVIEW 7\n.Os\n.Sh DESCRIPTION\n{}safe tail\n{}",
        ".Bl -tag -width key\n.It key\n".repeat(36),
        ".El\n".repeat(36)
    );
    let content = load_roff_bytes(source.as_bytes()).unwrap();
    let document = content.document.as_ref().unwrap();
    assert!(document.diagnostics.iter().any(|finding| {
        finding.code.as_deref() == Some("manual.document-structure-depth-summarized")
            && finding.impact == DiagnosticImpact::SemanticCoverage
    }));
    assert!(mant_ir::content_complete(&document.diagnostics));
    assert!(!mant_ir::semantics_complete(&document.diagnostics));
    let bundle = QueryBundle::from(&content);
    let encoded = serde_json::to_string(&bundle).unwrap();
    let decoded: QueryBundle = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, bundle);
    let decoded_content = mant_ir::ResolvedContent::from(decoded);
    let search = mant_query::search_query(
        &decoded_content,
        &SearchQuery {
            pattern: "tail".into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(search.total, 1);
    assert!(search.content_complete);
    assert!(search.diagnostics.iter().any(|finding| {
        finding.code.as_deref() == Some("manual.document-structure-depth-summarized")
    }));
}

#[test]
fn summarized_nested_operators_keep_parent_grouping_and_operands() {
    // Both exact 70-group inputs were checked with fixed CVS -Tutf8/-Thtml/
    // -Tlint: eqn.c::eqn_box_makebinary attaches the outer operator and
    // eqn_term.c preserves each explicit brace group around its base.
    for (operator, projected_operator) in [("over", " / "), ("sup", " ^ ")] {
        let expected = format!(
            "{}a{projected_operator}b{}{}c",
            "(".repeat(70),
            ")".repeat(70),
            projected_operator
        );
        let equation = format!(
            "{}a {operator} b{} {operator} c",
            "{ ".repeat(70),
            " }".repeat(70)
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
                matches!(block, Block::Equation { value, .. } if value == &expected)
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
