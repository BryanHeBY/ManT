//! Validate the same visible-flow boundaries that consume resolved gaps.
use super::{
    GapPlan, block_gap, compose_origin, coordinate, marker_run_in_gap,
    table_requires_origin_preserving_stack,
};
use crate::visit::Visit;
use crate::{Block, ContentContext, Inline, InlineView, ListKind};

/// Whether any resolved content boundary exceeds the presentation gap budget.
/// Transparent containers and zero-width anchors do not reset the boundary;
/// visible markers, terms and literal content do. Column-layout table cells
/// are independent flows; origin-preserving stacked cells share their parent flow.
/// This reports loss without allocating rendered text or blank rows.
#[must_use]
pub fn has_bounded_gap(content: ContentContext<'_>, blocks: &[Block]) -> bool {
    walk(content, blocks, &mut GapPlan::default(), 0, 0)
}

fn visible(content: ContentContext<'_>, nodes: &[Inline]) -> bool {
    let mut visitor = VisibleText {
        content,
        found: false,
    };
    nodes.iter().any(|node| {
        visitor.visit_inline(node);
        visitor.found
    })
}

// Style and semantic-name decoration cannot change whether a boundary has
// original visible text. Reuse the IR wrapper traversal without requesting
// presentation roles or allocating a rendered intermediate string.
struct VisibleText<'a> {
    content: ContentContext<'a>,
    found: bool,
}

impl<'ir> Visit<'ir> for VisibleText<'ir> {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if self.found {
            return;
        }
        match self.content.inline(inline) {
            Ok(InlineView::Text(value) | InlineView::Code(value)) => {
                self.found = !value.trim().is_empty();
            }
            Ok(InlineView::Strong(_) | InlineView::Emphasis(_) | InlineView::Link(_)) => {
                crate::visit::walk_inline(self, inline);
            }
            Ok(InlineView::LineBreak | InlineView::Anchor(_)) | Err(_) => {}
        }
    }
}

fn add(gap: &mut GapPlan, rows: u16) -> bool {
    gap.append_resolved(rows);
    gap.is_bounded()
}

// Keep the exhaustive boundary traversal together: splitting by variant must
// not silently replace the shared flow cursor at transparent containers.
#[allow(clippy::too_many_lines)]
fn walk(
    content: ContentContext<'_>,
    blocks: &[Block],
    gap: &mut GapPlan,
    depth: usize,
    origin: i32,
) -> bool {
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
                let origin = compose_origin(origin, layout.indent_columns);
                for (index, item) in items.iter().enumerate() {
                    if add(
                        gap,
                        item.layout
                            .spacing_before_lines
                            .unwrap_or(u16::from(index > 0 && !compact)),
                    ) {
                        return true;
                    }
                    let mut blocks = item.blocks.as_slice();
                    let marker_width = match kind {
                        ListKind::Plain => 0,
                        ListKind::Bullet => 2,
                        ListKind::Ordered { .. } => kind
                            .ordinal(index)
                            .map_or(0, |ordinal| ordinal.to_string().len() + 2),
                    };
                    if marker_width > 0 {
                        // Run-in paragraph spacing belongs before the marker.
                        if let Some(Block::Paragraph { layout, .. }) = blocks.first()
                            && marker_run_in_gap(origin, marker_width, layout.indent_columns)
                                .is_some()
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
                        content,
                        blocks,
                        gap,
                        depth + 1,
                        compose_origin(origin, coordinate(marker_width)),
                    ) {
                        return true;
                    }
                }
            }
            Block::DefinitionList {
                items,
                compact,
                layout,
                ..
            } => {
                let origin = compose_origin(origin, layout.indent_columns);
                for (index, item) in items.iter().enumerate() {
                    if add(
                        gap,
                        item.layout
                            .spacing_before_lines
                            .unwrap_or(u16::from(index > 0 && !compact)),
                    ) {
                        return true;
                    }
                    if item.terms.iter().any(|term| visible(content, term)) {
                        *gap = GapPlan::default();
                    }
                    if walk(
                        content,
                        &item.description,
                        gap,
                        depth + 1,
                        compose_origin(origin, item.layout.body_indent_columns),
                    ) {
                        return true;
                    }
                }
            }
            Block::Table { rows, layout, .. } => {
                let origin = compose_origin(origin, layout.indent_columns);
                let stack = table_requires_origin_preserving_stack(rows, origin);
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    let bounded = if stack {
                        walk(content, &cell.blocks, gap, depth + 1, origin)
                    } else {
                        walk(content, &cell.blocks, &mut GapPlan::default(), depth + 1, 0)
                    };
                    if bounded {
                        return true;
                    }
                }
                if !stack {
                    *gap = GapPlan::default();
                }
            }
            Block::Preformatted { children, .. } => {
                if content.has_literal_rows(children).unwrap_or(false) {
                    *gap = GapPlan::default();
                }
            }
            Block::FixedDisplay { view, .. } => {
                if content
                    .fixed_view(*view)
                    .is_some_and(|view| !view.lines.is_empty())
                {
                    *gap = GapPlan::default();
                }
            }
            Block::Paragraph { children, .. } => {
                if visible(content, children) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ContentFixture;
    use crate::{LayoutHint, ListItem, TableCell, TableRow};

    fn paragraph(indent: i32, gap: u16, children: Vec<Inline>) -> Block {
        Block::Paragraph {
            children,
            layout: LayoutHint {
                indent_columns: indent,
                spacing_before_lines: gap,
                ..Default::default()
            },
            source: None,
        }
    }

    fn text(fixture: &mut ContentFixture) -> Vec<Inline> {
        vec![fixture.text("BODY")]
    }

    #[test]
    fn paragraph_gap_boundaries_depend_on_original_text_not_inline_decoration() {
        let mut fixture = ContentFixture::body();
        let empty = fixture.text(String::new());
        let whitespace = fixture.code("\u{2003}\t");
        let break_only = fixture.hard_break();
        let anchor = fixture.anchor("target");
        let nested_break = fixture.hard_break();
        let space_link = fixture.link_text(
            crate::LinkTarget::External {
                uri: "https://example.test".into(),
            },
            None,
            " ",
            false,
        );
        let Inline::Link {
            occurrence: space_occurrence,
            children: space_children,
        } = space_link
        else {
            unreachable!();
        };
        let visible_link = fixture.link_text(
            crate::LinkTarget::External {
                uri: "https://example.test".into(),
            },
            None,
            "Cafe\u{301} 👩‍💻",
            false,
        );
        let body = text(&mut fixture);
        let store = fixture.finish();
        let content = store.content();
        for (children, bounded) in [
            (vec![empty], true),
            (vec![whitespace], true),
            (vec![break_only, anchor], true),
            (
                vec![Inline::Strong {
                    children: Vec::new(),
                }],
                true,
            ),
            (
                vec![Inline::Emphasis {
                    children: vec![nested_break],
                }],
                true,
            ),
            (
                vec![Inline::Link {
                    occurrence: space_occurrence,
                    children: vec![Inline::Strong {
                        children: space_children,
                    }],
                }],
                true,
            ),
            (
                vec![Inline::Strong {
                    children: vec![visible_link],
                }],
                false,
            ),
        ] {
            let description = format!("{children:?}");
            assert_eq!(
                has_bounded_gap(
                    content,
                    &[
                        Block::VerticalSpace {
                            lines: 3000,
                            source: None
                        },
                        paragraph(0, 0, children),
                        Block::VerticalSpace {
                            lines: 3000,
                            source: None
                        },
                        paragraph(0, 0, body.clone()),
                    ]
                ),
                bounded,
                "{description}"
            );
        }
    }

    #[test]
    fn a_literal_empty_row_resets_requests_but_a_target_does_not() {
        let mut fixture = ContentFixture::body();
        let empty_text = fixture.text(String::new());
        let empty_code = fixture.code(String::new());
        let nested_empty = fixture.text(String::new());
        let anchor = fixture.anchor("target");
        let body = text(&mut fixture);
        let store = fixture.finish();
        let content = store.content();
        for (children, bounded) in [
            (vec![empty_text], false),
            (vec![empty_code], false),
            (
                vec![Inline::Emphasis {
                    children: vec![nested_empty],
                }],
                false,
            ),
            (vec![anchor], true),
            (
                vec![Inline::Strong {
                    children: Vec::new(),
                }],
                true,
            ),
            (Vec::new(), true),
        ] {
            assert_eq!(
                has_bounded_gap(
                    content,
                    &[
                        Block::VerticalSpace {
                            lines: 3000,
                            source: None
                        },
                        Block::Preformatted {
                            children,
                            language: None,
                            layout: LayoutHint::default(),
                            source: None
                        },
                        Block::VerticalSpace {
                            lines: 3000,
                            source: None
                        },
                        paragraph(0, 0, body.clone()),
                    ]
                ),
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
        let mut fixture = ContentFixture::body();
        let body = text(&mut fixture);
        let store = fixture.finish();
        let content = store.content();
        for (origin, expected) in [(-2, true), (2, false)] {
            let table = Block::Table {
                fixed_view: None,
                rows: vec![TableRow {
                    kind: crate::TableRowKind::Data,
                    cells: vec![TableCell {
                        kind: crate::TableCellKind::Text,
                        blocks: vec![paragraph(3, 3000, body.clone())],
                        point: None,
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                        source: None,
                    }],
                }],
                layout: LayoutHint {
                    indent_columns: origin,
                    spacing_before_lines: 3000,
                    ..Default::default()
                },
                source: None,
            };
            assert_eq!(has_bounded_gap(content, &[table]), expected);
        }
    }

    #[test]
    fn stacked_marker_consumes_parent_gap_before_negative_child_gap() {
        let mut fixture = ContentFixture::body();
        let body = text(&mut fixture);
        let store = fixture.finish();
        let content = store.content();
        for (indent, expected) in [(-2, false), (2, true)] {
            assert_eq!(
                has_bounded_gap(
                    content,
                    &[bullet(3000, vec![paragraph(indent, 3000, body.clone())])]
                ),
                expected,
                "child indent {indent}"
            );
        }
    }

    #[test]
    fn marker_consumes_zero_width_first_paragraph_gap_exactly_once() {
        let mut fixture = ContentFixture::body();
        let anchor = fixture.anchor("empty");
        let body = text(&mut fixture);
        let store = fixture.finish();
        let context = store.content();
        let nodes = vec![
            paragraph(0, 3000, vec![anchor]),
            Block::VerticalSpace {
                lines: 2000,
                source: None,
            },
            paragraph(0, 0, body),
        ];
        // With no visible marker, these are one continuous boundary.
        assert!(has_bounded_gap(context, &nodes));
        // The bullet marker splits the two independent boundaries even when
        // its first paragraph contains only a zero-width target.
        assert!(!has_bounded_gap(context, &[bullet(0, nodes)]));
    }
}
