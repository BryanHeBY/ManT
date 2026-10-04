//! Validate the same visible-flow boundaries that consume resolved gaps.
use super::{
    GapPlan, block_gap, compose_origin, coordinate, list_item_spacing, list_marker_width,
    marker_run_in_gap, table_requires_origin_preserving_stack,
};
use crate::visit::Visit;
use crate::{Block, Inline};

/// Whether any resolved content boundary exceeds the presentation gap budget.
/// Transparent containers and zero-width anchors do not reset the boundary;
/// visible markers, terms and literal content do. Column-layout table cells
/// are independent flows; origin-preserving stacked cells share their parent flow.
/// This reports loss without allocating rendered text or blank rows.
#[must_use]
pub fn has_bounded_gap(blocks: &[Block]) -> bool {
    walk(blocks, &mut GapPlan::default(), 0, 0)
}

fn visible(nodes: &[Inline]) -> bool {
    let mut visitor = VisibleText(false);
    nodes.iter().any(|node| {
        visitor.visit_inline(node);
        visitor.0
    })
}

// Style and semantic-name decoration cannot change whether a boundary has
// original visible text. Reuse the IR wrapper traversal without requesting
// presentation roles or allocating a rendered intermediate string.
struct VisibleText(bool);

impl<'ir> Visit<'ir> for VisibleText {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if self.0 {
            return;
        }
        match inline {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                // Accepted author whitespace still owns a physical row.
                // Only a truly empty paragraph word contributes no output;
                // the source formatter already resolved invisible cell rows.
                self.0 = !value.is_empty();
            }
            Inline::Strong { .. } | Inline::Emphasis { .. } | Inline::Link { .. } => {
                crate::visit::walk_inline(self, inline);
            }
            Inline::LineBreak { .. } => self.0 = true,
            Inline::Anchor { .. } => {}
        }
    }
}

fn add(gap: &mut GapPlan, rows: u16) -> bool {
    gap.append_resolved(rows);
    gap.is_bounded()
}

// Container walkers borrow the active cursor; an owner change never creates
// an independent boundary unless the table layout explicitly requires one.
fn walk(blocks: &[Block], gap: &mut GapPlan, depth: usize, origin: i32) -> bool {
    // Invalid external IR is rejected by normal validation. Keep this helper
    // bounded even when invoked independently on unchecked in-memory data.
    if depth > 256 {
        return true;
    }
    for block in blocks {
        if add(gap, block_gap(block)) {
            return true;
        }
        match block {
            Block::VerticalSpace { .. } => {}
            Block::List {
                kind,
                items,
                compact,
                layout,
                ..
            } => {
                if walk_list(
                    *kind,
                    items,
                    *compact,
                    gap,
                    depth,
                    compose_origin(origin, layout.indent_columns),
                ) {
                    return true;
                }
            }
            Block::DefinitionList {
                items,
                compact,
                layout,
                ..
            } => {
                if walk_definitions(
                    items,
                    *compact,
                    gap,
                    depth,
                    compose_origin(origin, layout.indent_columns),
                ) {
                    return true;
                }
            }
            Block::Table { rows, layout, .. } => {
                if walk_table(
                    rows,
                    gap,
                    depth,
                    compose_origin(origin, layout.indent_columns),
                ) {
                    return true;
                }
            }
            Block::Preformatted { children, .. } => {
                if super::has_literal_rows(children) {
                    *gap = GapPlan::default();
                }
            }
            Block::Paragraph { children, .. } => {
                if visible(children) {
                    *gap = GapPlan::default();
                }
            }
            Block::Equation { value, .. } | Block::Unsupported { text: value, .. } => {
                if !value.trim().is_empty() {
                    *gap = GapPlan::default();
                }
            }
            Block::ThematicBreak { .. } => *gap = GapPlan::default(),
        }
    }
    false
}

fn walk_list(
    kind: crate::ListKind,
    items: &[crate::ListItem],
    compact: bool,
    gap: &mut GapPlan,
    depth: usize,
    origin: i32,
) -> bool {
    for (index, item) in items.iter().enumerate() {
        if add(
            gap,
            list_item_spacing(item.layout.spacing_before_lines, index, compact),
        ) {
            return true;
        }
        let mut blocks = item.blocks.as_slice();
        let marker_width = list_marker_width(kind, index);
        if marker_width > 0 {
            // Run-in paragraph spacing belongs before the marker.
            if let Some(Block::Paragraph { layout, .. }) = blocks.first()
                && marker_run_in_gap(origin, marker_width, layout.indent_columns).is_some()
            {
                if add(gap, layout.spacing_before_lines) {
                    return true;
                }
                *gap = GapPlan::default();
                // The visible marker consumes this boundary even
                // if the paragraph contains only zero-width anchors.
                blocks = &blocks[1..];
            } else {
                *gap = GapPlan::default();
            }
        }
        if walk(
            blocks,
            gap,
            depth + 1,
            compose_origin(origin, coordinate(marker_width)),
        ) {
            return true;
        }
    }
    false
}

fn walk_definitions(
    items: &[crate::DefinitionItem],
    compact: bool,
    gap: &mut GapPlan,
    depth: usize,
    origin: i32,
) -> bool {
    for (index, item) in items.iter().enumerate() {
        if add(
            gap,
            list_item_spacing(item.layout.spacing_before_lines, index, compact),
        ) {
            return true;
        }
        if item.terms.iter().any(|term| visible(term)) {
            *gap = GapPlan::default();
        }
        if walk(
            &item.description,
            gap,
            depth + 1,
            compose_origin(origin, item.layout.body_indent_columns),
        ) {
            return true;
        }
    }
    false
}

fn walk_table(rows: &[crate::TableRow], gap: &mut GapPlan, depth: usize, origin: i32) -> bool {
    let stack = table_requires_origin_preserving_stack(rows, origin);
    for row in rows {
        if crate::table_row_is_navigation_only(row) {
            continue;
        }
        if row.cells.is_empty() || !matches!(row.kind, crate::TableRowKind::Data) {
            // A structural blank row or a visible rule consumes
            // its incoming gap, including in source-order fallback.
            *gap = GapPlan::default();
        }
        for cell in &row.cells {
            let bounded = if stack {
                walk(&cell.blocks, gap, depth + 1, origin)
            } else {
                walk(&cell.blocks, &mut GapPlan::default(), depth + 1, 0)
            };
            if bounded {
                return true;
            }
            if cell.break_after || cell.kind != crate::TableCellKind::Text {
                // Closing an existing empty data row is a physical
                // contribution even when its blocks have no glyphs.
                *gap = GapPlan::default();
            }
        }
        // The table's existing data row remains a physical row
        // even when all of its cells have empty content roots.
        // A false cell boundary permits another cell to use this
        // row; it does not erase the row's structural ownership.
        *gap = GapPlan::default();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LayoutHint, ListItem, ListKind, TableCell, TableRow};

    fn paragraph(indent: i32, gap: u16, children: Vec<Inline>) -> Block {
        Block::Paragraph {
            inline_layout: crate::InlineLayout::default(),
            children,
            layout: LayoutHint {
                indent_columns: indent,
                spacing_before_lines: gap,
                ..Default::default()
            },
            source: None,
        }
    }

    fn text() -> Vec<Inline> {
        vec![Inline::Text {
            value: "BODY".into(),
        }]
    }

    #[test]
    fn paragraph_gap_boundaries_depend_on_original_text_not_inline_decoration() {
        for (children, bounded) in [
            (
                vec![Inline::Text {
                    value: String::new(),
                }],
                true,
            ),
            (
                vec![Inline::Code {
                    value: "\u{2003}\t".into(),
                }],
                false,
            ),
            (vec![Inline::line_break(), Inline::anchor("target")], false),
            (
                vec![Inline::Strong {
                    children: Vec::new(),
                }],
                true,
            ),
            (
                vec![Inline::Emphasis {
                    children: vec![Inline::LineBreak {}],
                }],
                false,
            ),
            (
                vec![Inline::Link {
                    title: None,
                    target: crate::LinkTarget::External {
                        uri: "https://example.test".into(),
                    },
                    children: vec![Inline::Strong {
                        children: vec![Inline::Text { value: " ".into() }],
                    }],
                }],
                false,
            ),
            (
                vec![Inline::Strong {
                    children: vec![Inline::Link {
                        title: None,
                        target: crate::LinkTarget::External {
                            uri: "https://example.test".into(),
                        },
                        children: vec![Inline::Emphasis {
                            children: vec![Inline::Text {
                                value: "Cafe\u{301} 👩‍💻".into(),
                            }],
                        }],
                    }],
                }],
                false,
            ),
        ] {
            let description = format!("{children:?}");
            assert_eq!(
                has_bounded_gap(&[
                    Block::VerticalSpace {
                        lines: 3000,
                        source: None
                    },
                    paragraph(0, 0, children),
                    Block::VerticalSpace {
                        lines: 3000,
                        source: None
                    },
                    paragraph(0, 0, text()),
                ]),
                bounded,
                "{description}"
            );
        }
    }

    #[test]
    fn a_literal_empty_row_resets_requests_but_a_target_does_not() {
        for (children, bounded) in [
            (
                vec![Inline::Text {
                    value: String::new(),
                }],
                false,
            ),
            (
                vec![Inline::Code {
                    value: String::new(),
                }],
                false,
            ),
            (
                vec![Inline::Emphasis {
                    children: vec![Inline::Text {
                        value: String::new(),
                    }],
                }],
                false,
            ),
            (vec![Inline::anchor("target")], true),
            (
                vec![Inline::Strong {
                    children: Vec::new(),
                }],
                true,
            ),
            (Vec::new(), true),
        ] {
            assert_eq!(
                has_bounded_gap(&[
                    Block::VerticalSpace {
                        lines: 3000,
                        source: None
                    },
                    Block::Preformatted {
                        inline_layout: crate::InlineLayout::default(),
                        children,
                        language: None,
                        layout: LayoutHint::default(),
                        source: None
                    },
                    Block::VerticalSpace {
                        lines: 3000,
                        source: None
                    },
                    paragraph(0, 0, text()),
                ]),
                bounded
            );
        }
    }

    fn bullet(gap: u16, blocks: Vec<Block>) -> Block {
        Block::List {
            kind: ListKind::Bullet,
            compact: true,
            items: vec![ListItem {
                layout: crate::ListItemLayout::default(),
                blocks,
                entry: None,
                source: None,
            }],
            layout: LayoutHint {
                spacing_before_lines: gap,
                ..Default::default()
            },
            source: None,
        }
    }

    #[test]
    fn signed_stacked_tables_share_the_gap_budget_with_their_parent() {
        for (origin, expected) in [(-2, true), (2, false)] {
            let table = Block::Table {
                column_preferences: crate::ColumnPreferences::default(),
                rows: vec![TableRow {
                    kind: crate::TableRowKind::Data,
                    cells: vec![TableCell {
                        break_after: false,
                        kind: crate::TableCellKind::Text,
                        blocks: vec![paragraph(3, 3000, text())],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    }],
                }],
                layout: LayoutHint {
                    indent_columns: origin,
                    spacing_before_lines: 3000,
                    ..Default::default()
                },
                source: None,
            };
            assert_eq!(has_bounded_gap(&[table]), expected);
        }
    }

    #[test]
    fn stacked_marker_consumes_parent_gap_before_negative_child_gap() {
        for (indent, expected) in [(-2, false), (2, true)] {
            assert_eq!(
                has_bounded_gap(&[bullet(3000, vec![paragraph(indent, 3000, text())])]),
                expected,
                "child indent {indent}"
            );
        }
    }

    #[test]
    fn marker_consumes_zero_width_first_paragraph_gap_exactly_once() {
        let content = vec![
            paragraph(0, 3000, vec![Inline::anchor("empty")]),
            Block::VerticalSpace {
                lines: 2000,
                source: None,
            },
            paragraph(0, 0, text()),
        ];
        // With no visible marker, these are one continuous boundary.
        assert!(has_bounded_gap(&content));
        // The bullet marker splits the two independent boundaries even when
        // its first paragraph contains only a zero-width target.
        assert!(!has_bounded_gap(&[bullet(0, content)]));
    }
}
