//! One source-aware block layout for plain and decorated text.
//! Decorators must preserve visible content and boundary whitespace.
use super::flow::Flow;
use super::indent_lines;
use crate::presentation::{EntryStyleMap, TextPresentation, TextRole, visit_inline_text};
use mant_ir::geometry::{compose_origin, coordinate, marker_run_in_gap, padding, text_width};
use mant_ir::{Block, DefinitionItem, Inline, ListItem, ListKind, Section, TableCell};

mod lists;

pub(super) struct BlockRenderer<'a> {
    pub(super) names: Option<EntryStyleMap<'a>>,
    pub(super) decorate: &'a dyn Fn(TextPresentation, &str) -> String,
    pub(super) locations: Option<&'a super::super::styles::LocatedStyles<'a>>,
}

impl BlockRenderer<'_> {
    pub(super) fn paint(&self, role: TextRole, text: &str) -> String {
        (self.decorate)(role.into(), text)
    }

    pub(super) fn inline_text(&self, children: &[Inline], role: TextRole) -> String {
        if let Some(locations) = self.locations {
            return locations.inline(children, role, self.decorate);
        }
        let mut text = String::new();
        let names = self
            .names
            .as_ref()
            .map_or(&[][..], |map| map.ranges(children));
        visit_inline_text(children, names, |inline, _, value| {
            text.push_str(&(self.decorate)(
                TextPresentation {
                    role,
                    inline,
                    matched: false,
                },
                value,
            ));
        });
        text
    }

    pub(super) fn sections_flow(&self, sections: &[Section], depth: usize) -> Flow {
        let mut output = Flow::default();
        for section in sections {
            output.extend(self.section_flow(section, depth));
        }
        output
    }

    pub(super) fn render_section(&self, section: &Section, depth: usize) -> String {
        self.section_flow(section, depth).finish(false)
    }

    fn section_flow(&self, section: &Section, depth: usize) -> Flow {
        let heading_indent = "  ".repeat(depth);
        let mut output = Flow::default();
        output.gap(section.spacing_before_lines);
        output.push_text(format!(
            "{heading_indent}{}",
            self.inline_text(&section.heading.content, TextRole::Heading)
        ));
        output.extend(self.block_flow(&section.blocks, coordinate(depth.saturating_mul(2))));
        output.extend(self.sections_flow(&section.children, depth + 1));
        output
    }

    pub(super) fn render_blocks(&self, blocks: &[Block], base_indent: i32) -> String {
        self.render_block_sequence(blocks, base_indent, None)
    }

    pub(super) fn render_block_sequence(
        &self,
        blocks: &[Block],
        base_indent: i32,
        leading_gap: Option<usize>,
    ) -> String {
        self.block_flow(blocks, base_indent)
            .finish(leading_gap.is_some())
    }

    pub(super) fn block_flow(&self, blocks: &[Block], base_indent: i32) -> Flow {
        let mut output = Flow::default();
        for block in blocks {
            output.gap(mant_ir::geometry::block_gap(block));
            output.extend(self.render_block(block, base_indent));
        }
        output
    }

    fn render_block(&self, block: &Block, base_indent: i32) -> Flow {
        // Literal newlines and whitespace are content, not layout requests.
        // They must survive even when the entire block contains only blanks.
        if let Block::Preformatted {
            children, layout, ..
        } = block
        {
            if !mant_ir::geometry::has_literal_rows(children) {
                return Flow::default();
            }
            return Flow::literal(indent_lines(
                &self.inline_text(children, TextRole::Body),
                padding(compose_origin(base_indent, layout.indent_columns)),
            ));
        }
        if let Block::Paragraph {
            children, layout, ..
        } = block
        {
            let value = self.inline_text(children, TextRole::Body);
            if value.trim().is_empty() {
                return Flow::default();
            }
            let first_origin = compose_origin(base_indent, layout.indent_columns);
            return Flow::text(
                value
                    .trim_matches('\n')
                    .split('\n')
                    .enumerate()
                    .map(|(index, line)| {
                        let origin = if index == 0 {
                            first_origin
                        } else {
                            compose_origin(first_origin, layout.continuation_indent_columns)
                        };
                        format!("{}{line}", " ".repeat(padding(origin)))
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        let (value, layout_indent) = match block {
            Block::Paragraph {
                children, layout, ..
            }
            | Block::Preformatted {
                children, layout, ..
            } => (
                self.inline_text(children, TextRole::Body),
                layout.indent_columns,
            ),
            Block::List {
                kind,
                items,
                compact,
                layout,
                ..
            } => {
                return self.render_list(
                    *kind,
                    items,
                    *compact,
                    compose_origin(base_indent, layout.indent_columns),
                );
            }
            Block::DefinitionList {
                items,
                compact,
                layout,
                ..
            } => {
                return self.render_definitions(
                    items,
                    *compact,
                    compose_origin(base_indent, layout.indent_columns),
                );
            }
            Block::Table { rows, layout, .. } => {
                let origin = compose_origin(base_indent, layout.indent_columns);
                if mant_ir::geometry::table_requires_origin_preserving_stack(rows, origin) {
                    return self.stacked_table_flow(rows, origin);
                }
                (
                    super::super::table::table_rows(rows, |cell| self.cell_text(cell)).join("\n"),
                    layout.indent_columns,
                )
            }
            Block::Equation { value, layout, .. }
            | Block::Unsupported {
                text: value,
                layout,
                ..
            } => (
                self.locations.map_or_else(
                    || self.paint(TextRole::Body, value),
                    |locations| locations.text(value, self.decorate),
                ),
                layout.indent_columns,
            ),
            // Vertical space is handled as an inter-block separator in
            // `render_blocks`, never as a standalone rendered block.
            Block::VerticalSpace { .. } => return Flow::default(),
            Block::ThematicBreak { .. } => ("---".to_owned(), 0),
        };
        Self::nonliteral_leaf(&value, compose_origin(base_indent, layout_indent))
    }

    fn nonliteral_leaf(value: &str, origin: i32) -> Flow {
        let value = value.trim_matches('\n');
        if value.trim().is_empty() {
            Flow::default()
        } else {
            Flow::text(indent_lines(value, padding(origin)))
        }
    }

    fn cell_text(&self, cell: &TableCell) -> String {
        self.render_blocks(&cell.blocks, 0).replace('\n', " ")
    }

    fn stacked_table_flow(&self, rows: &[mant_ir::TableRow], origin: i32) -> Flow {
        let mut output = Flow::default();
        for cell in rows.iter().flat_map(|row| &row.cells) {
            output.extend(self.block_flow(&cell.blocks, origin));
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::LayoutHint;

    #[test]
    fn literal_whitespace_is_content_even_at_indented_and_document_edges() {
        let renderer = super::super::plain_renderer();
        for value in ["", " ", "\n", "\n\n", "\nALPHA\n\n", "  \n \n"] {
            for origin in [0, 3] {
                let block = Block::Preformatted {
                    language: None,
                    children: vec![Inline::Text {
                        value: value.into(),
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                };
                let expected = value
                    .split('\n')
                    .map(|line| {
                        if line.is_empty() {
                            String::new()
                        } else {
                            format!("{}{line}", " ".repeat(origin))
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                assert_eq!(
                    renderer.render_blocks(&[block], i32::try_from(origin).unwrap()),
                    expected
                );
            }
        }
    }

    #[test]
    fn signed_table_cells_compose_parent_origins_before_clipping() {
        let renderer = super::super::plain_renderer();
        for (table_indent, child_indent, expected_column) in
            [(-2, 3, 1), (3, -2, 1), (4096, 3, 4096), (4090, 10, 4096)]
        {
            let cell = |text| TableCell {
                blocks: vec![paragraph(text, child_indent)],
                column_span: 2,
                row_span: 1,
                alignment: None,
            };
            let table = Block::Table {
                rows: vec![mant_ir::TableRow {
                    cells: vec![cell("FIRST"), cell("SECOND")],
                }],
                layout: LayoutHint {
                    indent_columns: table_indent,
                    ..Default::default()
                },
                source: None,
            };
            assert_eq!(
                renderer.render_blocks(&[table], 0),
                format!(
                    "{}FIRST\n{}SECOND",
                    " ".repeat(expected_column),
                    " ".repeat(expected_column)
                )
            );
        }
        let nested = plain_list(
            vec![Block::Table {
                rows: vec![mant_ir::TableRow {
                    cells: vec![TableCell {
                        blocks: vec![plain_list(vec![paragraph("NESTED", 5)], 3)],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    }],
                }],
                layout: LayoutHint {
                    indent_columns: 2,
                    ..Default::default()
                },
                source: None,
            }],
            4090,
        );
        assert_eq!(
            renderer.render_blocks(&[nested], 0),
            format!("{}NESTED", " ".repeat(4096))
        );
        let table = Block::Table {
            rows: vec![mant_ir::TableRow {
                cells: ["FIRST", "SECOND"]
                    .map(|text| TableCell {
                        blocks: vec![paragraph(text, 0)],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    })
                    .into(),
            }],
            layout: LayoutHint::default(),
            source: None,
        };
        let ordinary = renderer.render_blocks(&[table], 0);
        assert!(
            ordinary
                .lines()
                .any(|line| line.contains("FIRST") && line.contains("SECOND")),
            "{ordinary}"
        );
    }

    fn paragraph(text: &str, indent: i32) -> Block {
        Block::Paragraph {
            children: vec![Inline::Text { value: text.into() }],
            layout: LayoutHint {
                indent_columns: indent,
                ..Default::default()
            },
            source: None,
        }
    }

    fn plain_list(blocks: Vec<Block>, indent: i32) -> Block {
        Block::List {
            kind: ListKind::Plain,
            compact: true,
            items: vec![ListItem {
                layout: mant_ir::ListItemLayout::default(),
                blocks,
                source: None,
                entry: None,
            }],
            layout: LayoutHint {
                indent_columns: indent,
                ..Default::default()
            },
            source: None,
        }
    }

    #[test]
    fn resolved_gaps_cross_transparent_containers_and_precede_whole_items() {
        let renderer = super::super::plain_renderer();
        for rows in [0, 1, 2] {
            let mut body = paragraph("BODY\nNEXT", 0);
            if let Block::Paragraph { layout, .. } = &mut body {
                layout.spacing_before_lines = rows;
            }
            let mut list = plain_list(vec![body], 0);
            if let Block::List { kind, .. } = &mut list {
                *kind = ListKind::Bullet;
            }
            assert_eq!(
                renderer.render_blocks(&[list], 0),
                format!("{}- BODY\n  NEXT", "\n".repeat(usize::from(rows)))
            );
        }
        let mut container = plain_list(
            vec![
                Block::VerticalSpace {
                    lines: 3000,
                    source: None,
                },
                paragraph("AFTER", 0),
            ],
            0,
        );
        if let Block::List { layout, .. } = &mut container {
            layout.spacing_before_lines = 3000;
        }
        let blocks = [paragraph("BEFORE", 0), container];
        assert!(mant_ir::geometry::has_bounded_gap(&blocks));
        assert_eq!(
            renderer.render_blocks(&blocks, 0),
            format!("BEFORE{}AFTER", "\n".repeat(4097))
        );
    }

    #[test]
    fn subtree_translation_is_applied_once_at_each_visible_leaf() {
        let blocks = vec![
            paragraph("PROSE", 0),
            plain_list(
                vec![
                    paragraph("CHILD", 0),
                    plain_list(vec![paragraph("DEEP", 1)], 2),
                    Block::DefinitionList {
                        declaration_groups: vec![],
                        compact: false,
                        items: vec![DefinitionItem {
                            terms: vec![vec![Inline::Text {
                                value: "TERM".into(),
                            }]],
                            description: vec![paragraph("BODY", -2)],
                            source: None,
                            entry: None,
                            layout: mant_ir::DefinitionLayout::default(),
                        }],
                        layout: LayoutHint::default(),
                        source: None,
                    },
                    Block::Table {
                        rows: vec![mant_ir::TableRow {
                            cells: vec![TableCell {
                                blocks: vec![paragraph("CELL", 1)],
                                column_span: 1,
                                row_span: 1,
                                alignment: None,
                            }],
                        }],
                        layout: LayoutHint::default(),
                        source: None,
                    },
                ],
                3,
            ),
        ];
        let original = blocks.clone();
        let renderer = super::super::plain_renderer();
        let baseline = renderer.render_blocks(&blocks, 0);
        for shift in [0, 2, 5] {
            assert_eq!(
                renderer.render_blocks(&blocks, shift),
                indent_lines(&baseline, padding(shift))
            );
        }
        assert_eq!(
            blocks, original,
            "presentation cannot compensate by mutating IR"
        );
        assert!(baseline.lines().any(|line| line == "      DEEP"));
        assert!(baseline.lines().any(|line| line == "     BODY"));
        assert_eq!(
            renderer.render_blocks(&[plain_list(vec![paragraph("OUTDENT", 3)], -2)], 0),
            " OUTDENT"
        );
    }

    #[test]
    fn distinct_term_roots_and_hard_lines_do_not_acquire_commas() {
        let blocks = [Block::DefinitionList {
            declaration_groups: vec![],
            compact: true,
            items: vec![DefinitionItem {
                terms: ["-a", "--all"]
                    .map(|value| {
                        vec![Inline::Text {
                            value: value.into(),
                        }]
                    })
                    .into(),
                description: vec![paragraph("FIRST\nCONTINUATION", 0)],
                source: None,
                entry: None,
                layout: mant_ir::DefinitionLayout {
                    inline_term: true,
                    ..Default::default()
                },
            }],
            layout: LayoutHint::default(),
            source: None,
        }];
        assert_eq!(
            super::super::plain_renderer().render_blocks(&blocks, 0),
            "-a\n--all FIRST\n    CONTINUATION"
        );
    }

    #[test]
    fn zero_width_terms_and_clipped_markers_do_not_move_body_text() {
        let renderer = super::super::plain_renderer();
        let mut block = plain_list(vec![paragraph("BODY", 4)], -5);
        let Block::List { kind, .. } = &mut block else {
            unreachable!()
        };
        *kind = ListKind::Bullet;
        assert_eq!(renderer.render_blocks(&[block], 0), "-\n BODY");
        let block = Block::DefinitionList {
            declaration_groups: vec![],
            compact: true,
            items: vec![DefinitionItem {
                source: None,
                entry: None,
                terms: vec![
                    vec![Inline::anchor_at("target", None)],
                    vec![Inline::Text {
                        value: "TERM".into(),
                    }],
                ],
                description: vec![paragraph("BODY", 0)],
                layout: mant_ir::DefinitionLayout {
                    inline_term: true,
                    ..Default::default()
                },
            }],
            layout: LayoutHint::default(),
            source: None,
        };
        assert_eq!(renderer.render_blocks(&[block], 0), "TERM BODY");
    }
}
