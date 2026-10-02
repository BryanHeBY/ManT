//! Native tbl/eqn payloads keep one source owner through public consumers.
#![cfg(feature = "roff")]

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_codec::encode::{MarkdownOptions, render_addressable_markdown_with_options};
use mant_ir::{Block, DefinitionItem, Inline, ResolvedContent, inline_plain_text, visit};
use mant_protocol::{
    ContentSelector, ExplanationBlockStep, ExplanationContent, ExplanationContentRange,
    ExplanationOptions, ExplanationQuery, QueryBundle, SearchQuery, SearchScope,
};
use serde_json::Value;

fn fixtures() -> Value {
    let fixture: Value = serde_json::from_str(include_str!("native_payload_consumers/cases.json"))
        .expect("permanent source and oracle bindings");
    assert_eq!(fixture["count"], 9);
    assert_eq!(fixture["expectations_from_product"], false);
    assert_eq!(
        fixture["oracle_sha256"],
        "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6"
    );
    fixture
}

fn text<'a>(case: &'a Value, key: &str) -> &'a str {
    case[key].as_str().expect("explicit fixture text")
}

fn nodes<'a>(node: &'a Node, selected: &mut Vec<&'a Node>, predicate: fn(&Node) -> bool) {
    if predicate(node) {
        selected.push(node);
    }
    for child in &node.children {
        nodes(child, selected, predicate);
    }
}

fn contains_text(node: &Node, value: &str) -> bool {
    node.text.as_deref() == Some(value)
        || node
            .children
            .iter()
            .any(|child| contains_text(child, value))
}

fn assert_native_owner(case: &Value) {
    let native = Parser::default()
        .parse_bytes("payload.1", text(case, "source").as_bytes())
        .unwrap();
    let mut definitions = Vec::new();
    nodes(&native.document.root, &mut definitions, |node| {
        node.kind == NodeKind::Block && matches!(node.macro_name.as_deref(), Some("It" | "TP"))
    });
    let [owner] = definitions.as_slice() else {
        panic!("one source declaration: {}", case["id"]);
    };
    assert_eq!(owner.macro_name.as_deref(), Some(text(case, "owner_macro")));
    assert_eq!(u64::from(owner.line), case["owner_line"].as_u64().unwrap());
    let head = owner
        .children
        .iter()
        .find(|node| node.kind == NodeKind::Head)
        .unwrap();
    let body = owner
        .children
        .iter()
        .find(|node| node.kind == NodeKind::Body)
        .unwrap();
    assert!(!contains_text(head, "TailWord"));
    assert!(contains_text(body, "TailWord"));
    let mut payloads = Vec::new();
    nodes(body, &mut payloads, |node| {
        !node.table_cells.is_empty()
            || node.equation.as_ref().is_some_and(|equation| {
                // The exact tree profile has an empty configuration EQ at
                // line 10 and an actual inline expression at line 13.
                !equation.children.is_empty() || equation.text.is_some()
            })
    });
    let lines: Vec<_> = payloads.iter().map(|node| node.line).collect();
    let expected: Vec<_> = case["native_payload_lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| u32::try_from(line.as_u64().unwrap()).unwrap())
        .collect();
    assert_eq!(lines, expected, "{}: actual BODY lineage", case["id"]);
    if text(case, "kind") == "tbl" {
        let cells: Vec<Vec<_>> = payloads
            .iter()
            .map(|node| {
                node.table_cells
                    .iter()
                    .map(|cell| cell.text.as_deref().unwrap_or_default())
                    .collect()
            })
            .collect();
        assert_eq!(serde_json::to_value(cells).unwrap(), case["native_cells"]);
    }
}

fn round_trip(case: &Value) -> ResolvedContent {
    let source = mant_loader::load_roff_bytes(text(case, "source").as_bytes()).unwrap();
    let json = mant_render::render_query_json(&source, false).unwrap();
    assert!(!json.contains("\\u0000mant:"));
    let decoded: QueryBundle = serde_json::from_str(&json).unwrap();
    let decoded: ResolvedContent = decoded.into();
    assert_eq!(decoded, source);
    assert_eq!(
        mant_ir::validate_document(decoded.document.as_ref().unwrap()).len(),
        0
    );
    decoded
}

fn owner(content: &ResolvedContent) -> &DefinitionItem {
    let section = content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap();
    let [Block::DefinitionList { items, .. }] = section.blocks.as_slice() else {
        panic!("source definition container: {:?}", section.blocks);
    };
    let [item] = items.as_slice() else {
        panic!("one source owner");
    };
    item
}

fn block_text(block: &Block) -> String {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            inline_plain_text(children)
        }
        Block::Equation { value, .. } => value.clone(),
        _ => panic!("one text root: {block:?}"),
    }
}

fn assert_table(case: &Value, blocks: &[Block]) {
    let [Block::Table { rows, .. }, Block::Paragraph { .. }] = blocks else {
        panic!("finalized table and independent tail: {blocks:?}");
    };
    let cells: Vec<Vec<_>> = rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| {
                    let [block] = cell.blocks.as_slice() else {
                        panic!("one cell root")
                    };
                    block_text(block)
                })
                .collect()
        })
        .collect();
    let spans: Vec<Vec<_>> = rows
        .iter()
        .map(|row| row.cells.iter().map(|cell| cell.column_span).collect())
        .collect();
    assert_eq!(serde_json::to_value(cells).unwrap(), case["cells"]);
    assert_eq!(serde_json::to_value(spans).unwrap(), case["spans"]);
    if text(case, "id") == "table_restart" {
        assert_eq!(
            rows[1].cells[0].alignment,
            Some(mant_ir::TableAlignment::Right)
        );
    }
    if text(case, "id") == "table_fonts" {
        let Block::Paragraph { children, .. } = &rows[0].cells[0].blocks[0] else {
            unreachable!()
        };
        assert!(
            matches!(children.as_slice(), [Inline::Strong { children: first }, Inline::Text { value }, Inline::Strong { children: last }]
            if inline_plain_text(first) == "A" && value == "B" && inline_plain_text(last) == "C")
        );
        let Block::Paragraph { children, .. } = &rows[0].cells[1].blocks[0] else {
            unreachable!()
        };
        assert!(
            matches!(children.as_slice(), [Inline::Emphasis { children }] if inline_plain_text(children) == "STYLEITALIC")
        );
    }
}

#[derive(Default)]
struct ReaderPayloads {
    fences: Vec<String>,
    codes: Vec<String>,
}
impl<'ir> visit::Visit<'ir> for ReaderPayloads {
    fn visit_block(&mut self, block: &'ir Block) {
        if let Block::Preformatted { children, .. } = block {
            self.fences.push(inline_plain_text(children));
        }
        visit::walk_block(self, block);
    }
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Code { value } = inline {
            self.codes.push(value.clone());
        }
        visit::walk_inline(self, inline);
    }
}

fn assert_cell_references(content: &ResolvedContent) {
    let inventory = mant_query::project_references(
        content.document.as_ref().unwrap(),
        None,
        mant_ir::ReferenceScope::Document,
        &mant_protocol::ReferenceProjection {
            mode: mant_protocol::ReferenceProjectionMode::All,
            // `All` materializes the selected types; the default selection
            // includes document/manual references, not external URIs.
            target_types: vec![
                mant_ir::ReferenceTargetType::External,
                mant_ir::ReferenceTargetType::Manual,
            ],
            ..Default::default()
        },
    );
    assert_eq!(inventory.records.len(), 3);
    let records = &inventory.records;
    assert_eq!(
        records
            .iter()
            .map(|record| record.label.as_str())
            .collect::<Vec<_>>(),
        ["CELLONE", "CELLTWO", "printf(3)"]
    );
    assert_eq!(
        records[0].target,
        mant_ir::LinkTarget::External {
            uri: "https://example.org".into()
        }
    );
    assert_eq!(records[0].target, records[1].target);
    assert_ne!(records[0].origin, records[1].origin);
    assert_eq!(
        records[2].target,
        mant_ir::LinkTarget::Manual {
            name: "printf".into(),
            manual_section: Some("3".into()),
        }
    );
    for record in records {
        let excerpt =
            mant_query::select_excerpt(content, std::slice::from_ref(&record.source_read)).unwrap();
        assert!(mant_render::render_excerpt_text(&excerpt).contains(&record.label));
    }
}

fn assert_reader(case: &Value, content: &ResolvedContent) {
    use visit::Visit as _;
    let markdown = mant_codec::encode::render_markdown(content);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let mut observed = ReaderPayloads::default();
    observed.visit_document(readback.document.as_ref().unwrap());
    let expected = text(case, "reader_payload");
    if text(case, "kind") == "inline-eqn" {
        assert_eq!(observed.fences.len(), 0);
        assert_eq!(observed.codes, [expected]);
    } else {
        assert_eq!(observed.fences, [expected]);
    }
    let visible = mant_render::render_query_text(&readback);
    for word in case["words"].as_array().unwrap() {
        assert_eq!(
            visible.matches(word.as_str().unwrap()).count(),
            1,
            "{}: reader {visible:?}",
            case["id"]
        );
    }
}

fn assert_search(case: &Value, content: &ResolvedContent) {
    let artifact = render_addressable_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        let result = mant_query::search_query(
            content,
            &SearchQuery {
                pattern: text(case, "lookup").into(),
                scope,
                syntax: mant_protocol::SearchSyntax::Literal,
                case: mant_protocol::SearchCase::Sensitive,
                word: false,
                context_lines: 0,
                limit: 10,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(result.total, 1, "{}: {scope:?}", case["id"]);
        assert!(result.content_complete);
        let found = &result.matches[0];
        assert_eq!(found.occurrences.len(), 1, "one exact artifact range");
        assert_eq!(found.occurrence_count, 1);
        assert!(!found.occurrences_truncated);
        assert_eq!(found.outline.path(), "2/e1");
        assert_eq!(
            found.node_source.unwrap().line,
            u32::try_from(case["owner_line"].as_u64().unwrap()).unwrap()
        );
        for occurrence in &found.occurrences {
            assert_eq!(occurrence.matched_text, text(case, "lookup"));
            let bytes = usize::try_from(occurrence.markdown.start_byte).unwrap()
                ..usize::try_from(occurrence.markdown.end_byte).unwrap();
            assert_eq!(
                artifact.text().get(bytes.clone()),
                Some(text(case, "lookup"))
            );
            // Search keeps artifact bytes and one-based scalar columns as
            // separate contracts. Greek operands exercise their difference.
            for (offset, line, column) in [
                (
                    bytes.start,
                    occurrence.markdown.start_line,
                    occurrence.markdown.start_column,
                ),
                (
                    bytes.end,
                    occurrence.markdown.end_line,
                    occurrence.markdown.end_column,
                ),
            ] {
                let prefix = &artifact.text()[..offset];
                assert_eq!(
                    usize::try_from(line).unwrap(),
                    prefix.bytes().filter(|byte| *byte == b'\n').count() + 1
                );
                assert_eq!(
                    usize::try_from(column).unwrap(),
                    prefix.rsplit('\n').next().unwrap().chars().count() + 1
                );
            }
        }
    }
}

fn assert_explanation(case: &Value, content: &ResolvedContent) {
    let explanation = mant_query::explain_query(
        content,
        &ExplanationQuery {
            entry: text(case, "lookup").into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let wire = serde_json::to_string(&explanation).unwrap();
    let decoded: mant_protocol::QueryExplanation = serde_json::from_str(&wire).unwrap();
    assert_eq!(decoded, explanation);
    assert!(decoded.content_complete);
    assert_eq!(
        decoded.semantics_complete,
        !case["summarized"].as_bool().unwrap()
    );
    let mut matches = 0;
    for evidence in &decoded.evidence {
        assert_eq!(evidence.outline.path(), "2/e1");
        let Some(ExplanationContent::Entry { block } | ExplanationContent::Block { block }) =
            &evidence.content
        else {
            panic!("one materialized source owner: {evidence:?}");
        };
        for preview in &evidence.previews {
            for range in &preview.content_ranges {
                let root = range.resolve(block).expect("returned scalar location");
                let selected = root
                    .safe_text()
                    .chars()
                    .skip(range.char_range().start)
                    .take(range.char_range().len())
                    .collect::<String>();
                assert_eq!(selected, text(case, "lookup"));
                assert_eq!(
                    u64::try_from(range.char_range().start).unwrap(),
                    case["lookup_start"].as_u64().unwrap()
                );
                if let Some(cell) = case["lookup_cell"].as_array() {
                    let ExplanationContentRange::BlockText { path, .. } = range else {
                        panic!("cell block root")
                    };
                    assert!(path.contains(&ExplanationBlockStep::TableCell {
                        row: u32::try_from(cell[0].as_u64().unwrap()).unwrap(),
                        column: u32::try_from(cell[1].as_u64().unwrap()).unwrap(),
                    }));
                    let mut stale = range.clone();
                    let ExplanationContentRange::BlockText { path, .. } = &mut stale else {
                        unreachable!()
                    };
                    for step in path {
                        if let ExplanationBlockStep::TableCell { column, .. } = step {
                            *column = u32::MAX;
                        }
                    }
                    assert!(
                        stale.resolve(block).is_none(),
                        "stale cell cannot search for a replacement"
                    );
                }
                matches += 1;
            }
        }
    }
    assert_eq!(
        matches, 1,
        "{}: one exact returned literal occurrence",
        case["id"]
    );
}

#[test]
fn finalized_table_payloads_keep_semantics_reader_rows_and_owner_locations() {
    // Each exact source ran pristine ASCII/UTF-8/HTML/tree/lint before these
    // expectations. tbl_term.c::tbl_word scopes font registers; tbl_data.c
    // determines cells/spans and T&. The Lk/Xr source has actual UNSUPP lint=4:
    // documented bounded T{} recovery enriches the proven operand cell only.
    let fixture = fixtures();
    for case in fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| text(case, "kind") == "tbl")
    {
        assert_native_owner(case);
        let content = round_trip(case);
        let item = owner(&content);
        assert_eq!(
            item.entry.as_ref().unwrap().names,
            [text(case, "owner_name")]
        );
        assert_table(case, &item.description);
        if text(case, "id") == "table_recovered_links" {
            assert_cell_references(&content);
        }
        assert_reader(case, &content);
        assert_search(case, &content);
        assert_explanation(case, &content);
    }
}

#[test]
fn structural_equations_keep_fences_scalars_and_safe_depth_summaries() {
    // eqn_term.c::eqn_box/eqn_html.c::eqn_box independently retain authored
    // fences and nested operand groups. All five pristine profiles accept
    // these exact sources. sqrt(...) and matrix row separators are the
    // already-selected readable contract, not device geometry. Structural
    // depth summaries keep every accepted operand without content loss.
    let fixture = fixtures();
    for case in fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| text(case, "kind") != "tbl")
    {
        assert_native_owner(case);
        let content = round_trip(case);
        let item = owner(&content);
        assert_eq!(
            item.entry.as_ref().unwrap().names,
            [text(case, "owner_name")]
        );
        if text(case, "kind") == "inline-eqn" {
            let [Block::Paragraph { children, .. }] = item.description.as_slice() else {
                panic!("inline equation container")
            };
            let values: Vec<_> = children
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Equation { value, expression } => {
                        assert_eq!(*value, expression.readable_text());
                        Some(value.as_str())
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(values, [text(case, "equation")]);
        } else {
            let [
                Block::Equation {
                    value,
                    expression: Some(expression),
                    ..
                },
                Block::Paragraph { .. },
            ] = item.description.as_slice()
            else {
                panic!("display equation container")
            };
            assert_eq!(value, text(case, "equation"));
            assert_eq!(expression.readable_text(), text(case, "equation"));
        }
        assert_reader(case, &content);
        assert_search(case, &content);
        assert_explanation(case, &content);
        let excerpt =
            mant_query::select_excerpt(&content, &[ContentSelector::path("2/e1")]).unwrap();
        assert!(mant_render::render_excerpt_text(&excerpt).contains("TailWord"));
        assert!(!mant_render::render_excerpt_text(&excerpt).contains("END"));
    }
}
