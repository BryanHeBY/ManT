//! Match native tbl owners by source intervals, not all semantic Table nodes.

use mant_ir::{Block, Inline, LinkTarget, ResolvedContent, TableRowKind, visit::Visit};

use super::fixture;

#[derive(Default)]
struct Tables<'a>(Vec<&'a Block>);

impl<'a> Visit<'a> for Tables<'a> {
    fn visit_block(&mut self, block: &'a Block) {
        if matches!(block, Block::Table { .. }) {
            self.0.push(block);
        }
        mant_ir::visit::walk_block(self, block);
    }
}

#[derive(Default)]
struct Text(String);

impl Visit<'_> for Text {
    fn visit_inline(&mut self, inline: &Inline) {
        match inline {
            Inline::Text { value } | Inline::Code { value } => self.0.push_str(value),
            Inline::LineBreak { .. } => self.0.push('\n'),
            _ => mant_ir::visit::walk_inline(self, inline),
        }
    }
}

fn text(blocks: &[Block]) -> String {
    let mut visitor = Text::default();
    for block in blocks {
        visitor.visit_block(block);
    }
    visitor.0
}

#[derive(Default)]
struct Semantics {
    emphasis: bool,
    manual_names: Vec<String>,
}

impl Visit<'_> for Semantics {
    fn visit_inline(&mut self, inline: &Inline) {
        match inline {
            Inline::Emphasis { .. } => self.emphasis = true,
            Inline::Link {
                target:
                    LinkTarget::Manual {
                        name,
                        manual_section,
                    },
                ..
            } => {
                assert_eq!(manual_section.as_deref(), Some("3"));
                self.manual_names.push(name.clone());
            }
            _ => {}
        }
        mant_ir::visit::walk_inline(self, inline);
    }
}

fn assert_cell_semantics(rows: &[mant_ir::TableRow], case: &fixture::Case, table_index: usize) {
    for semantic in case
        .expected
        .cell_semantics
        .iter()
        .filter(|cell| cell.table == table_index)
    {
        let mut actual = Semantics::default();
        for block in &rows[semantic.row].cells[semantic.column].blocks {
            actual.visit_block(block);
        }
        assert_eq!(
            actual.emphasis, semantic.emphasis,
            "{}: cell recovery transaction",
            case.id
        );
        assert_eq!(
            actual.manual_names,
            semantic.manual_name.iter().cloned().collect::<Vec<_>>(),
            "{}: original typed cell target",
            case.id
        );
    }
}

pub(super) fn assert_tables(content: &ResolvedContent, case: &fixture::Case) {
    let mut tables = Tables::default();
    tables.visit_document(content.document.as_ref().unwrap());
    for (table_index, expected) in case.expected.table_owners.iter().enumerate() {
        let owners = tables
            .0
            .iter()
            .filter(|table| {
                matches!(table, Block::Table { source: Some(source), .. }
                    if source.line > expected.ts_line && source.line < expected.te_line)
            })
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(owners.len(), 1, "{}: one original TS/TE owner", case.id);
        let Block::Table {
            rows,
            column_count,
            source,
            ..
        } = owners[0]
        else {
            unreachable!()
        };
        assert_eq!(
            source.unwrap().line,
            expected.first_span_line,
            "{}",
            case.id
        );
        assert_eq!(
            *column_count,
            Some(u32::from(expected.declared_columns)),
            "{}",
            case.id
        );
        assert_eq!(rows.len(), expected.rows.len(), "{}: all spans", case.id);
        assert_cell_semantics(rows, case, table_index);
        for (row, expected_row) in rows.iter().zip(&expected.rows) {
            let kind = match &row.kind {
                TableRowKind::Data => "data",
                TableRowKind::HorizontalRule => "horizontal-rule",
                TableRowKind::DoubleHorizontalRule => "double-horizontal-rule",
                TableRowKind::LayoutRule { .. } => "layout-rule",
            };
            assert_eq!(kind, expected_row.kind, "{}: original span kind", case.id);
            assert_eq!(row.cells.len(), expected_row.cells.len(), "{}", case.id);
            for (index, (cell, expected_text)) in
                row.cells.iter().zip(&expected_row.cells).enumerate()
            {
                assert_eq!(text(&cell.blocks), *expected_text, "{}", case.id);
                if let Some(span) = expected_row.cell_spans.get(index) {
                    assert_eq!(cell.column_span, span.column_span, "{}", case.id);
                    assert_eq!(cell.row_span, span.row_span, "{}", case.id);
                }
                for block in &cell.blocks {
                    let Block::Paragraph { source, .. } = block else {
                        panic!("{}: original cell paragraph", case.id)
                    };
                    assert_eq!(
                        source.unwrap().line,
                        expected_row.source_line,
                        "{}: native cell's original row",
                        case.id
                    );
                }
            }
        }
    }
}
