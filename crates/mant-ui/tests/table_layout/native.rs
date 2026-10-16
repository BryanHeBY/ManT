use crate::table_matrix::*;
use mant_ir::{Block, TableRow, TableRowKind, visit::Visit};
use mant_ui::DocumentView;
use serde_json::Value;

const CASES: &str = include_str!("../../../../tests/fixtures/roff/table_layout/cases.json");
const SOURCES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/roff/table_layout"
);

struct Tables<'a>(Vec<&'a Block>);
impl<'a> Visit<'a> for Tables<'a> {
    fn visit_block(&mut self, block: &'a Block) {
        if matches!(block, Block::Table { .. }) {
            self.0.push(block);
        }
        mant_ir::visit::walk_block(self, block);
    }
}

#[test]
fn recorded_native_table_facts_survive_json_and_every_reader_width() {
    // Every complete input ran through the locked pristine reference before
    // recording expectations. tbl_opts/layout/data, tbl_term and tbl_html
    // independently establish authored edges, widths, rows and spans.
    let cases: Value = serde_json::from_str(CASES).unwrap();
    assert_eq!(cases["cases"].as_array().unwrap().len(), 18);
    for case in cases["cases"].as_array().unwrap() {
        let name = case["file"].as_str().unwrap();
        let source = std::fs::read_to_string(format!("{SOURCES}/{name}")).unwrap();
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let value = restored(&loaded);
        let mut tables = Tables(vec![]);
        tables.visit_document(value.document.as_ref().unwrap());
        assert_eq!(
            tables.0.len(),
            usize::try_from(case["tables"].as_u64().unwrap()).unwrap(),
            "{name}"
        );
        let facts = case["facts"].as_array().unwrap();
        assert_eq!(tables.0.len(), facts.len());
        for (table, expected) in tables.0.iter().zip(facts) {
            assert_table_facts(table, expected, name);
        }
        assert_source_consumers(&value, case);
    }
}

fn assert_table_facts(table: &Block, expected: &Value, name: &str) {
    let Block::Table {
        alignment,
        borders,
        column_count,
        rows,
        ..
    } = table
    else {
        unreachable!()
    };
    assert_eq!(
        column_count.map(u64::from),
        expected["columns"].as_u64(),
        "{name}"
    );
    assert_eq!(
        serde_json::to_value(borders).unwrap(),
        expected["borders"],
        "{name}"
    );
    if let Some(expected_alignment) = expected.get("alignment") {
        assert_eq!(
            serde_json::to_value(alignment).unwrap(),
            *expected_alignment,
            "{name}"
        );
    }
    if let Some(width) = expected["preferredWidthColumns"].as_u64() {
        let count = rows
            .iter()
            .flat_map(|row| &row.cells)
            .filter(|cell| cell.preferred_width_columns.map(u64::from) == Some(width))
            .count();
        if let Some(expected_count) = expected["preferredWidthCount"].as_u64() {
            assert_eq!(
                count,
                usize::try_from(expected_count).unwrap(),
                "{name}: both width-bearing columns must survive"
            );
        } else {
            assert!(count > 0, "{name}: lost width evidence");
        }
    }
    for (key, row_span) in [("columnSpan", false), ("rowSpan", true)] {
        if let Some(span) = expected[key].as_u64() {
            assert!(
                rows.iter()
                    .flat_map(|row| &row.cells)
                    .any(|cell| u64::from(if row_span {
                        cell.row_span
                    } else {
                        cell.column_span
                    }) == span),
                "{name}: lost {key}"
            );
        }
    }
    assert_eq!(
        rows.iter().any(|row| !row.column_borders.is_empty()),
        expected["rowEdges"].as_bool().unwrap(),
        "{name}: row-local edges and defaults are independent"
    );
    if let Some(edges) = expected.get("rowColumnBorders") {
        assert_eq!(
            serde_json::to_value(
                rows.iter()
                    .map(|row| &row.column_borders)
                    .collect::<Vec<_>>()
            )
            .unwrap(),
            *edges,
            "{name}"
        );
    }
    assert_row_facts(rows, expected, name);
}

fn assert_row_facts(rows: &[TableRow], expected: &Value, name: &str) {
    assert_eq!(
        rows.len(),
        usize::try_from(expected["rowCount"].as_u64().unwrap()).unwrap(),
        "{name}: physical source rows"
    );
    let mut kinds = std::collections::BTreeMap::new();
    for row in rows {
        let kind = match row.kind {
            TableRowKind::Data => "data",
            TableRowKind::HorizontalRule => "horizontal-rule",
            TableRowKind::DoubleHorizontalRule => "double-horizontal-rule",
            TableRowKind::LayoutRule { .. } => "layout-rule",
        };
        *kinds.entry(kind).or_insert(0_usize) += 1;
    }
    assert_eq!(
        serde_json::to_value(kinds).unwrap(),
        expected["rowKinds"],
        "{name}: every authored rule must survive"
    );
}

fn assert_source_consumers(value: &mant_ir::ResolvedContent, case: &Value) {
    let name = case["file"].as_str().unwrap();
    let markers = case["markers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|marker| marker.as_str().unwrap())
        .collect::<Vec<_>>();
    let view = DocumentView::new(value);
    for width in [40_u16, 80, 120, 24, 80] {
        let rendered = view.render(width);
        for marker in &markers {
            assert_eq!(
                rendered.search(marker).len(),
                1,
                "{name}, {width}, {marker}: {:?}",
                rendered.text
            );
        }
        assert_buffer(&rendered, width, &markers);
        assert_no_decoration_search(&rendered);
        if let Some(forbidden) = case["forbidden"].as_array() {
            for forbidden in forbidden {
                assert!(
                    rendered.search(forbidden.as_str().unwrap()).is_empty(),
                    "{name}: rejected native rule payload revived"
                );
            }
        }
    }
    assert_markers(value, 80, &markers);
    let markdown = mant_codec::encode::render_markdown(value);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let text = mant_render::render_query_text(&readback);
    for marker in markers {
        assert_eq!(text.matches(marker).count(), 1, "{name}: {markdown}");
    }
    if let Some(forbidden) = case["forbidden"].as_array() {
        for forbidden in forbidden {
            assert!(
                !text.contains(forbidden.as_str().unwrap()),
                "{name}: rejected rule payload in export"
            );
        }
    }
}
