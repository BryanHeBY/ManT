//! Decide when cell-local column rendering cannot preserve resolved origins.
use super::{compose_origin, coordinate, padding};
use crate::{Block, ListKind, TableRow};

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
        match block {
            Block::List { kind, items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    let marker = match kind {
                        ListKind::Plain => 0,
                        ListKind::Bullet => 2,
                        ListKind::Ordered { .. } => {
                            kind.ordinal(index).map_or(0, |n| n.to_string().len() + 2)
                        }
                    };
                    let body = compose_origin(origin, coordinate(marker));
                    if clips(body) {
                        return true;
                    }
                    pending.extend(item.blocks.iter().map(|block| (block, body)));
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
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

fn clips(origin: i32) -> bool {
    coordinate(padding(origin)) != origin
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LayoutHint, TableCell};

    fn rows(block: Block) -> Vec<TableRow> {
        vec![TableRow {
            cells: vec![TableCell {
                blocks: vec![block],
                column_span: 1,
                row_span: 1,
                alignment: None,
            }],
        }]
    }

    fn paragraph(indent: i32, continuation: i32) -> Block {
        Block::Paragraph {
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
                rows: rows(paragraph(indent, continuation)),
                layout: LayoutHint::default(),
                source: None,
            });
            assert!(table_requires_origin_preserving_stack(&nested, 7));
        }
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
                terms: vec![],
                description: vec![paragraph(0, 0)],
                entry: None,
                source: None,
                layout: crate::DefinitionLayout {
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
}
