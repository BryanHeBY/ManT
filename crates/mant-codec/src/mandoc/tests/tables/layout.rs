//! Native frame, layout-edge and soft-width facts reach source-neutral IR.
use super::*;
use mant_ir::TableBorderStyle::{Double, Single};

fn fixture(name: &str) -> Option<mant_ir::Document> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/table_layout")
        .join(format!("{name}.1"));
    if !path.exists() {
        return None; // The standalone published package omits repository fixtures.
    }
    Some(parse_manual_bytes(&path, &std::fs::read(&path).unwrap()).unwrap())
}

fn first_table(document: &mant_ir::Document) -> &Block {
    document.sections[0]
        .blocks
        .iter()
        .find(|block| matches!(block, Block::Table { .. }))
        .unwrap()
}

#[test]
fn row_local_edges_keep_strengths_and_restart_ownership() {
    // Exact fixtures first checked with pristine tree/ascii/utf8/html/lint.
    // tbl_term.c consumes layout->vert and cp->vert independently of data.
    let Some(document) = fixture("vertical-boundaries") else {
        return;
    };
    let Block::Table { borders, rows, .. } = first_table(&document) else {
        unreachable!()
    };
    assert!(borders.is_empty());
    let edge_facts = |row: &mant_ir::TableRow| {
        row.column_borders
            .iter()
            .map(|edge| (edge.column, edge.style))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        edge_facts(&rows[0]),
        [(0, Single), (1, Double), (2, Single)]
    );
    assert_eq!(
        edge_facts(&rows[1]),
        [(0, Double), (1, Single), (2, Double)]
    );
    assert_eq!(rows[0].cells.len(), 2);
    assert_eq!(rows[1].cells.len(), 2);
    let json = serde_json::to_string(&document).unwrap();
    let restored: mant_ir::Document = serde_json::from_str(&json).unwrap();
    assert_eq!(document, restored);
}

#[test]
fn frames_and_allbox_do_not_copy_glyphs_or_duplicate_span_owners() {
    // tbl_opts.c aliases frame/box and doubleframe/doublebox. tbl_term.c's
    // tbl_hrule skips covered span interiors; these stay topology, not text.
    for (name, outer) in [
        ("frame-alias", Single),
        ("doublebox", Double),
        ("allbox-spans", Single),
    ] {
        let Some(document) = fixture(name) else {
            return;
        };
        let Block::Table {
            borders,
            rows,
            column_preferences,
            ..
        } = first_table(&document)
        else {
            unreachable!()
        };
        assert_eq!(borders.outer, Some(outer));
        assert_eq!(
            borders.inner_rows,
            (name == "allbox-spans").then_some(Single)
        );
        assert_eq!(
            borders.inner_columns,
            (name == "allbox-spans").then_some(Single)
        );
        assert!(
            column_preferences.widths.is_empty(),
            "native tbl never selects sequential mdoc fields"
        );
        if name == "allbox-spans" {
            assert_eq!(rows[0].cells[0].column_span, 2);
            assert_eq!(rows[1].cells[0].row_span, 2);
            assert_eq!(rows[2].cells[0].blocks, []);
            assert!(rows.iter().all(|row| row.column_borders.is_empty()));
        }
    }
}

#[test]
fn soft_cell_widths_do_not_split_the_authoritative_text() {
    // out.c::tblcalc_literal uses w() as word-wrap capacity but retains long
    // words. Consumer wrapping is viewport dependent, not producer text.
    for (name, expected) in [("width-units", 10), ("unbreakable-width", 4)] {
        let Some(document) = fixture(name) else {
            return;
        };
        let Block::Table {
            rows,
            column_preferences,
            ..
        } = first_table(&document)
        else {
            unreachable!()
        };
        assert_eq!(rows[0].cells[0].preferred_width_columns, Some(expected));
        assert_eq!(column_preferences.widths, [] as [u16; 0]);
        if name == "unbreakable-width" {
            let Block::Paragraph { children, .. } = &rows[0].cells[0].blocks[0] else {
                unreachable!()
            };
            assert_eq!(inline_text(children), "UNBREAKABLE_LONG_TOKEN");
            assert!(
                !children
                    .iter()
                    .any(|inline| matches!(inline, Inline::LineBreak { .. }))
            );
        }
    }
    let Some(document) = fixture("boxed-escape-paragraphs") else {
        return;
    };
    let Block::Table { rows, .. } = first_table(&document) else {
        unreachable!()
    };
    let data: Vec<_> = rows
        .iter()
        .filter(|row| {
            row.cells
                .get(1)
                .is_some_and(|cell| cell.preferred_width_columns == Some(34))
        })
        .collect();
    assert_eq!(data.len(), 4);
    for (index, row) in data.iter().enumerate() {
        for (column, marker) in [(1, "DESC_END"), (2, "MEAN_END")] {
            let Block::Paragraph { children, .. } = &row.cells[column].blocks[0] else {
                unreachable!()
            };
            assert!(inline_text(children).contains(&format!("{marker}_{index}")));
            assert_eq!(row.cells[column].preferred_width_columns, Some(34));
        }
    }
}

#[test]
fn derived_border_budget_reduces_one_complete_table_without_losing_body() {
    // This exact generated source passed pristine -Ttree/-Tlint before the
    // assertion: 512 declared slots, 512 actual data owners, 511 authored
    // inner edges shared by one native layout. No giant screen render needed.
    let mut source = format!(
        ".TH PROBE 1 \"October 16, 2026\"\n.SH DESCRIPTION\n.TS\nbox;\n{}.\n",
        vec!["l"; 512].join(" | ")
    );
    for index in 0..512 {
        writeln!(&mut source, "ROW_{index}").unwrap();
    }
    source.push_str(".TE\n");
    let document =
        parse_manual_bytes(std::path::Path::new("edge-budget.1"), source.as_bytes()).unwrap();
    let Block::Table {
        borders,
        rows,
        column_count,
        ..
    } = first_table(&document)
    else {
        unreachable!()
    };
    assert_eq!(*column_count, Some(512));
    assert_eq!(borders.outer, Some(mant_ir::TableBorderStyle::Single));
    assert_eq!(rows.len(), 512);
    for (index, row) in rows.iter().enumerate() {
        assert!(
            row.column_borders.is_empty(),
            "reduction applies to the whole table"
        );
        assert_eq!(row.cells.len(), 1);
        let Block::Paragraph { children, .. } = &row.cells[0].blocks[0] else {
            unreachable!()
        };
        assert_eq!(inline_text(children), format!("ROW_{index}"));
    }
    let diagnostics: Vec<_> = document
        .diagnostics
        .iter()
        .filter(|finding| finding.code.as_deref() == Some("manual.table-layout-reduced"))
        .collect();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].impact, mant_ir::DiagnosticImpact::None);
    assert_eq!(mant_ir::validate_document(&document), []);
}
