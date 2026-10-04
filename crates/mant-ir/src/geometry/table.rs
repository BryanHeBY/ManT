//! Decide when cell-local column rendering cannot preserve resolved origins.
use super::{compose_origin, coordinate, list_marker_width, padding};
use crate::{Block, TableCell, TableRow};

/// Operation-local extrema of a cell's relative visual origins. This contains
/// no body, source execution history, or serialized document state.
#[derive(Clone, Copy, Debug, Default)]
pub struct CellOriginBounds {
    minimum: i32,
    maximum: i32,
    negative_displacement: bool,
    parent_sensitive: bool,
}

impl CellOriginBounds {
    fn observe(&mut self, origin: i32) {
        self.minimum = self.minimum.min(origin);
        self.maximum = self.maximum.max(origin);
    }

    fn inline(&mut self, layout: &crate::InlineLayout, origin: i32, hanging: i32) {
        for hint in &layout.row_hints {
            self.negative_displacement |= hint.indent_columns < 0;
            self.observe(compose_origin(origin, hint.indent_columns));
            self.observe(compose_origin(
                compose_origin(origin, hanging),
                hint.indent_columns,
            ));
        }
    }

    /// Whether every visual origin fits after composing the actual parent.
    /// Author text extent is independent of these generated display origins.
    #[must_use]
    pub fn fits_at(self, origin: i32) -> bool {
        !clips(compose_origin(origin, self.minimum)) && !clips(compose_origin(origin, self.maximum))
    }
}

/// Whether a table needs source-order cells rendered at the actual parent.
/// Negative displacements conservatively retain their original owner context.
#[must_use]
pub fn table_requires_origin_preserving_stack(rows: &[TableRow], origin: i32) -> bool {
    clips(origin)
        || rows.iter().flat_map(|row| &row.cells).any(|cell| {
            let bounds = table_cell_origin_bounds(cell);
            bounds.negative_displacement || bounds.parent_sensitive || !bounds.fits_at(origin)
        })
}

/// Collect cell origin facts once, before viewport wrapping or field placement.
/// Pinned `mdoc_term.c::termp_bd_pre` composes offsets with the active parent;
/// consumers similarly compose parent, actual field, and child before clipping.
#[must_use]
pub fn table_cell_origin_bounds(cell: &TableCell) -> CellOriginBounds {
    let mut bounds = CellOriginBounds::default();
    let mut pending = cell
        .blocks
        .iter()
        .map(|block| (block, 0))
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
        bounds.negative_displacement |=
            layout.indent_columns < 0 || layout.continuation_indent_columns < 0;
        bounds.observe(origin);
        bounds.observe(compose_origin(origin, layout.continuation_indent_columns));
        if let Block::Paragraph { inline_layout, .. } | Block::Preformatted { inline_layout, .. } =
            block
        {
            bounds.inline(inline_layout, origin, layout.continuation_indent_columns);
        }
        match block {
            Block::List { kind, items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    let body = compose_origin(origin, coordinate(list_marker_width(*kind, index)));
                    bounds.observe(body);
                    pending.extend(item.blocks.iter().map(|block| (block, body)));
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    for term in &item.terms {
                        bounds.inline(&term.inline_layout, origin, 0);
                    }
                    let body = compose_origin(origin, item.layout.body_indent_columns);
                    bounds.negative_displacement |= item.layout.body_indent_columns < 0;
                    bounds.observe(body);
                    pending.extend(item.description.iter().map(|block| (block, body)));
                }
            }
            Block::Table { rows, .. } => {
                bounds.parent_sensitive = true;
                pending.extend(
                    rows.iter()
                        .flat_map(|row| &row.cells)
                        .flat_map(|cell| &cell.blocks)
                        .map(|block| (block, origin)),
                );
            }
            _ => {}
        }
    }
    bounds
}

/// Check actual field starts in one pass, then each cell's cached extrema once.
/// A long cell never causes a scan of its blocks per physical output row.
#[must_use]
pub fn table_column_origins_fit(
    bounds: &[CellOriginBounds],
    placements: &[Vec<super::ColumnPiece>],
    parent: i32,
) -> bool {
    let mut maximum = vec![None::<usize>; bounds.len()];
    for piece in placements.iter().flatten() {
        let Some(start) = maximum.get_mut(piece.cell) else {
            return false;
        };
        *start = Some(start.map_or(piece.column, |previous| previous.max(piece.column)));
    }
    bounds.iter().zip(maximum).all(|(bounds, start)| {
        start.is_none_or(|start| bounds.fits_at(compose_origin(parent, coordinate(start))))
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
                break_after: false,
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
                column_preferences: crate::ColumnPreferences::default(),
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
            column_preferences: crate::ColumnPreferences::default(),
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
    #[test]
    fn actual_field_parent_and_child_origins_are_checked_together() {
        let cell = |indent, correction| {
            rows(Block::Paragraph {
                children: vec![crate::Inline::Text { value: "B".into() }],
                inline_layout: crate::InlineLayout {
                    row_hints: if correction == 0 {
                        vec![]
                    } else {
                        vec![crate::RowLayoutHint {
                            row: 0,
                            indent_columns: correction,
                        }]
                    },
                },
                layout: LayoutHint {
                    indent_columns: indent,
                    ..Default::default()
                },
                source: None,
            })
            .remove(0)
            .cells
            .remove(0)
        };
        let placements = vec![vec![super::super::ColumnPiece {
            cell: 0,
            line: 0,
            column: 6,
        }]];
        for (indent, correction, fits) in [(0, 0, true), (2, 0, false), (0, 2, false)] {
            let bounds = table_cell_origin_bounds(&cell(indent, correction));
            assert!(bounds.fits_at(4090));
            assert_eq!(table_column_origins_fit(&[bounds], &placements, 4090), fits);
        }
        // Both hard-row first origins and soft continuations contribute to
        // extrema; a correction retains the hanging increment.
        let mut hanging = cell(0, 2);
        if let Block::Paragraph { layout, .. } = &mut hanging.blocks[0] {
            layout.continuation_indent_columns = 2;
        }
        let bounds = table_cell_origin_bounds(&hanging);
        assert!(bounds.fits_at(4092));
        assert!(!bounds.fits_at(4093));
        assert!(table_requires_origin_preserving_stack(
            &rows(Block::Paragraph {
                children: vec![crate::Inline::Text {
                    value: "OUTDENT".into()
                }],
                inline_layout: crate::InlineLayout::default(),
                layout: LayoutHint {
                    indent_columns: -2,
                    ..Default::default()
                },
                source: None,
            }),
            5
        ));
    }
}
