//! Decide when cell-local column rendering cannot preserve resolved origins.
use super::{compose_origin, coordinate, list_marker_width, padding};
use crate::{Block, TableRow};

/// Whether a table must render its cells in source order at the real parent
/// origin, rather than first laying each cell out at local column zero.
///
/// An origin outside the final padding bounds, a negative displacement, or a
/// descendant crossing the bounds cannot survive clipping a cell independently
/// and then translating its already-rendered text. This
/// conservative fallback preserves blocks, links, anchors, and relative
/// offsets; ordinary in-range nonnegative tables retain their column layout. The scan
/// is iterative so callers do not add recursive traversal depth here.
#[must_use]
pub fn table_requires_origin_preserving_stack(rows: &[TableRow], origin: i32) -> bool {
    if clips(origin) {
        return true;
    }
    let mut pending = rows
        .iter()
        .flat_map(|row| &row.cells)
        .flat_map(|cell| &cell.blocks)
        .map(|block| (block, origin))
        .collect::<Vec<_>>();
    while let Some((block, parent)) = pending.pop() {
        let layout = match block {
            Block::Paragraph { layout, .. }
            | Block::Preformatted { layout, .. }
            | Block::Equation { layout, .. }
            | Block::Unsupported { layout, .. }
            | Block::List { layout, .. }
            | Block::DefinitionList { layout, .. }
            | Block::Table { layout, .. } => layout,
            Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => continue,
        };
        let origin = compose_origin(parent, layout.indent_columns);
        if layout.indent_columns < 0
            || layout.continuation_indent_columns < 0
            || clips(origin)
            || clips(compose_origin(origin, layout.continuation_indent_columns))
        {
            return true;
        }
        if let Block::Paragraph { inline_layout, .. } | Block::Preformatted { inline_layout, .. } =
            block
            && hints_require_stack(inline_layout, origin, layout.continuation_indent_columns)
        {
            return true;
        }
        match block {
            Block::List { kind, items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    let marker = list_marker_width(*kind, index);
                    let body = compose_origin(origin, coordinate(marker));
                    if clips(body) {
                        return true;
                    }
                    pending.extend(item.blocks.iter().map(|block| (block, body)));
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    if item
                        .terms
                        .iter()
                        .any(|term| hints_require_stack(&term.inline_layout, origin, 0))
                    {
                        return true;
                    }
                    let body = compose_origin(origin, item.layout.body_indent_columns);
                    if item.layout.body_indent_columns < 0 || clips(body) {
                        return true;
                    }
                    pending.extend(item.description.iter().map(|block| (block, body)));
                }
            }
            Block::Table { rows, .. } => pending.extend(
                rows.iter()
                    .flat_map(|row| &row.cells)
                    .flat_map(|cell| &cell.blocks)
                    .map(|block| (block, origin)),
            ),
            _ => {}
        }
    }
    false
}

fn hints_require_stack(layout: &crate::InlineLayout, origin: i32, hanging: i32) -> bool {
    layout.row_hints.iter().any(|hint| {
        hint.indent_columns < 0
            || clips(compose_origin(origin, hint.indent_columns))
            || clips(compose_origin(
                compose_origin(origin, hanging),
                hint.indent_columns,
            ))
    })
}

fn clips(origin: i32) -> bool {
    coordinate(padding(origin)) != origin
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LayoutHint, ListKind, TableCell};

    fn rows(block: Block) -> Vec<TableRow> {
        vec![TableRow {
            kind: crate::TableRowKind::Data,
            cells: vec![TableCell {
                kind: crate::TableCellKind::Text,
                blocks: vec![block],
                column_span: 1,
                row_span: 1,
                alignment: None,
            }],
        }]
    }

    fn paragraph(indent: i32, continuation: i32) -> Block {
        Block::Paragraph {
            inline_layout: crate::InlineLayout::default(),
            children: vec![],
            layout: LayoutHint {
                indent_columns: indent,
                continuation_indent_columns: continuation,
                ..Default::default()
            },
            source: None,
        }
    }

    #[test]
    fn negative_origins_and_nested_outdents_require_stacking() {
        let positive = rows(paragraph(3, 0));
        assert!(!table_requires_origin_preserving_stack(&positive, 0));
        assert!(!table_requires_origin_preserving_stack(&positive, 7));
        assert!(table_requires_origin_preserving_stack(&positive, -2));
        for (indent, continuation) in [(-2, 0), (3, -2)] {
            let nested = rows(Block::Table {
                column_widths: Vec::new(),
                rows: rows(paragraph(indent, continuation)),
                layout: LayoutHint::default(),
                source: None,
            });
            assert!(table_requires_origin_preserving_stack(&nested, 7));
        }
    }

    #[test]
    fn each_ordered_item_uses_its_own_marker_width_at_the_padding_bound() {
        let list = |count| {
            rows(Block::List {
                kind: ListKind::Ordered { start: Some(9) },
                compact: true,
                items: (0..count)
                    .map(|_| crate::ListItem {
                        layout: crate::ListItemLayout::default(),
                        blocks: vec![paragraph(0, 0)],
                        entry: None,
                        source: None,
                    })
                    .collect(),
                layout: LayoutHint::default(),
                source: None,
            })
        };
        // At parent 4093, the first "9. " reaches 4096. The next "10. "
        // reaches 4097, which cannot survive independent cell clipping.
        assert!(!table_requires_origin_preserving_stack(&list(1), 4093));
        assert!(table_requires_origin_preserving_stack(&list(2), 4093));
    }

    #[test]
    fn composed_padding_limits_include_nested_markers_and_definition_bodies() {
        assert!(!table_requires_origin_preserving_stack(
            &rows(paragraph(6, 0)),
            4090
        ));
        assert!(table_requires_origin_preserving_stack(
            &rows(paragraph(7, 0)),
            4090
        ));
        assert!(table_requires_origin_preserving_stack(
            &rows(paragraph(0, 7)),
            4090
        ));
        assert!(table_requires_origin_preserving_stack(
            &rows(paragraph(3, 0)),
            4096
        ));
        let nested = rows(Block::Table {
            column_widths: Vec::new(),
            rows: rows(paragraph(4, 0)),
            layout: LayoutHint {
                indent_columns: 3,
                ..Default::default()
            },
            source: None,
        });
        assert!(table_requires_origin_preserving_stack(&nested, 4090));
        for kind in [
            ListKind::Bullet,
            ListKind::Ordered {
                start: Some(u64::MAX),
            },
        ] {
            let list = rows(Block::List {
                kind,
                compact: true,
                items: vec![crate::ListItem {
                    layout: crate::ListItemLayout::default(),
                    blocks: vec![paragraph(0, 0)],
                    entry: None,
                    source: None,
                }],
                layout: LayoutHint::default(),
                source: None,
            });
            assert!(table_requires_origin_preserving_stack(&list, 4095));
            assert!(!table_requires_origin_preserving_stack(&list, 4000));
        }
        let definitions = rows(Block::DefinitionList {
            declaration_groups: vec![],
            items: vec![crate::DefinitionItem {
                head_body_relation: crate::HeadBodyRelation::Separate,
                terms: Vec::new(),
                description: vec![paragraph(0, 0)],
                entry: None,
                source: None,
                layout: crate::DefinitionLayout {
                    body_alignment: crate::DefinitionBodyAlignment::Indented,
                    body_indent_columns: 10,
                    ..Default::default()
                },
            }],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        });
        assert!(table_requires_origin_preserving_stack(&definitions, 4090));
        assert!(!table_requires_origin_preserving_stack(&definitions, 4086));
    }

    #[test]
    fn owner_row_hints_preserve_signed_parent_composition_before_cell_clipping() {
        let inline_layout = |correction| crate::InlineLayout {
            row_hints: vec![crate::RowLayoutHint {
                row: 0,
                indent_columns: correction,
            }],
        };
        let paragraph = |correction, hanging| Block::Paragraph {
            children: vec![crate::Inline::Text {
                value: "BODY".into(),
            }],
            inline_layout: inline_layout(correction),
            layout: LayoutHint {
                continuation_indent_columns: hanging,
                ..Default::default()
            },
            source: None,
        };
        // Local clipping of -3 followed by parent +5 would produce 5;
        // resolving the real parent first correctly places the glyph at 2.
        assert!(table_requires_origin_preserving_stack(
            &rows(paragraph(-3, 0)),
            5
        ));
        assert!(!table_requires_origin_preserving_stack(
            &rows(paragraph(3, 4)),
            5
        ));
        assert!(!table_requires_origin_preserving_stack(
            &rows(paragraph(6, 0)),
            4090
        ));
        assert!(table_requires_origin_preserving_stack(
            &rows(paragraph(7, 0)),
            4090
        ));
        assert!(table_requires_origin_preserving_stack(
            &rows(paragraph(3, 4)),
            4090
        ));

        let literal = Block::Preformatted {
            children: vec![crate::Inline::Text {
                value: "FIRST\nSECOND".into(),
            }],
            inline_layout: crate::InlineLayout {
                row_hints: vec![crate::RowLayoutHint {
                    row: 1,
                    indent_columns: -2,
                }],
            },
            language: None,
            layout: LayoutHint::default(),
            source: None,
        };
        assert!(table_requires_origin_preserving_stack(&rows(literal), 5));
    }

    #[test]
    fn definition_term_hints_participate_in_table_origin_fallback() {
        let definition = |correction| Block::DefinitionList {
            items: vec![crate::DefinitionItem {
                head_body_relation: crate::HeadBodyRelation::Separate,
                terms: vec![crate::DefinitionTerm {
                    content: vec![crate::Inline::Text {
                        value: "TERM".into(),
                    }],
                    inline_layout: crate::InlineLayout {
                        row_hints: vec![crate::RowLayoutHint {
                            row: 0,
                            indent_columns: correction,
                        }],
                    },
                }],
                description: vec![],
                layout: crate::DefinitionLayout::default(),
                entry: None,
                source: None,
            }],
            compact: true,
            declaration_groups: vec![],
            layout: LayoutHint::default(),
            source: None,
        };
        assert!(table_requires_origin_preserving_stack(
            &rows(definition(-3)),
            5
        ));
        assert!(!table_requires_origin_preserving_stack(
            &rows(definition(3)),
            5
        ));
        assert!(!table_requires_origin_preserving_stack(
            &rows(definition(6)),
            4090
        ));
        assert!(table_requires_origin_preserving_stack(
            &rows(definition(7)),
            4090
        ));
    }
}
