//! One source-aware block layout for plain and decorated text.
//! Decorators must preserve visible content and boundary whitespace.
use super::flow::Flow;
use super::{indent_lines, join_parts};
use mant_ir::{Block, DefinitionItem, Inline, ListItem, ListKind, Section, TableCell};
use mant_protocol::geometry::{compose_origin, coordinate, marker_run_in_gap, padding, text_width};
use mant_protocol::{EntryStyleMap, TextPresentation, TextRole, visit_inline_text};

pub(super) struct BlockRenderer<'a> {
    pub(super) names: Option<EntryStyleMap<'a>>,
    pub(super) decorate: &'a dyn Fn(TextPresentation, &str) -> String,
    pub(super) locations: Option<&'a super::super::explanation::spans::LocatedStyles<'a>>,
}

impl BlockRenderer<'_> {
    pub(super) fn paint(&self, role: TextRole, text: &str) -> String {
        (self.decorate)(role.into(), text)
    }

    fn inline_text(&self, children: &[Inline], role: TextRole) -> String {
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

    pub(super) fn render_sections(&self, sections: &[Section], depth: usize) -> String {
        sections
            .iter()
            .map(|section| self.render_section(section, depth))
            .filter(|section| !section.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub(super) fn render_section(&self, section: &Section, depth: usize) -> String {
        let heading_indent = "  ".repeat(depth);
        let mut parts = vec![format!(
            "{heading_indent}{}",
            self.paint(TextRole::Heading, &section.title)
        )];
        let blocks = self.render_blocks(&section.blocks, coordinate(depth.saturating_mul(2)));
        if !blocks.is_empty() {
            parts.push(blocks);
        }
        let children = self.render_sections(&section.children, depth + 1);
        if !children.is_empty() {
            parts.push(children);
        }
        join_parts(parts)
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

    fn block_flow(&self, blocks: &[Block], base_indent: i32) -> Flow {
        let mut output = Flow::default();
        for block in blocks {
            output.gap(mant_protocol::geometry::block_gap(block));
            output.extend(self.render_block(block, base_indent));
        }
        output
    }

    fn render_block(&self, block: &Block, base_indent: i32) -> Flow {
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
                if mant_protocol::geometry::table_requires_origin_preserving_stack(rows, origin) {
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
        let value = value.trim_matches('\n');
        if value.trim().is_empty() {
            Flow::default()
        } else {
            Flow::text(indent_lines(
                value,
                padding(compose_origin(base_indent, layout_indent)),
            ))
        }
    }

    fn render_list(
        &self,
        kind: ListKind,
        items: &[ListItem],
        compact: bool,
        base_indent: i32,
    ) -> Flow {
        let mut output = Flow::default();
        for (index, item) in items.iter().enumerate() {
            if index > 0 && !compact {
                output.gap(1);
            }
            let marker = match kind {
                ListKind::Ordered { .. } => {
                    format!("{}. ", kind.ordinal(index).expect("ordered list ordinal"))
                }
                ListKind::Bullet => "- ".to_owned(),
                ListKind::Plain => String::new(),
            };
            let body_origin = compose_origin(base_indent, coordinate(text_width(&marker)));
            if marker.is_empty() {
                output.extend(self.block_flow(&item.blocks, body_origin));
                continue;
            }
            let prefix = format!("{}{marker}", " ".repeat(padding(base_indent)));
            if let Some(first @ Block::Paragraph { layout, .. }) = item.blocks.first()
                && let Some(gap) =
                    marker_run_in_gap(base_indent, text_width(&marker), layout.indent_columns)
            {
                // The first paragraph's boundary precedes the whole item,
                // not the text after its marker. Keep it out of string
                // prefix tests and preserve subsequent hard-line origins.
                output.gap(layout.spacing_before_lines);
                let body = self.render_block(first, body_origin).finish(false);
                let indent =
                    " ".repeat(padding(compose_origin(body_origin, layout.indent_columns)));
                let rest = body.strip_prefix(&indent).unwrap_or(&body);
                output.push_text(format!("{prefix}{}{rest}", " ".repeat(gap)));
                output.extend(self.block_flow(&item.blocks[1..], body_origin));
            } else {
                output.push_text(prefix.trim_end().to_owned());
                output.extend(self.block_flow(&item.blocks, body_origin));
            }
        }
        output
    }

    fn render_definitions(
        &self,
        items: &[DefinitionItem],
        compact: bool,
        base_indent: i32,
    ) -> Flow {
        let mut output = Flow::default();
        for (index, item) in items.iter().enumerate() {
            output.gap(
                item.layout
                    .spacing_before_lines
                    .unwrap_or(u16::from(index > 0 && !compact)),
            );
            output.extend(self.render_definition(item, base_indent));
        }
        output
    }

    fn render_definition(&self, item: &DefinitionItem, origin: i32) -> Flow {
        let body_origin = compose_origin(origin, item.layout.body_indent_columns);
        let mut terms = item
            .terms
            .iter()
            .map(|term| self.inline_text(term, TextRole::DefinitionTerm))
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();
        if let Some((children, layout)) = item.inline_description()
            && let Some(last) = terms.pop()
        {
            let last_plain = item
                .terms
                .iter()
                .rev()
                .find(|term| !crate::inline::plain_text(term).is_empty())
                .map(|term| crate::inline::plain_text(term))
                .unwrap_or_default();
            let last_width = text_width(last_plain.rsplit('\n').next().unwrap_or_default());
            let first_origin =
                compose_origin(body_origin, layout.indent_columns).max(compose_origin(
                    origin,
                    coordinate(
                        last_width.saturating_add(usize::from(item.layout.min_term_gap_columns)),
                    ),
                ));
            let body = self.inline_text(children, TextRole::Body);
            let mut lines = body.split('\n');
            let mut output = terms
                .into_iter()
                .map(|term| indent_lines(&term, padding(origin)))
                .collect::<Vec<_>>();
            output.push(format!(
                "{}{}{}",
                indent_lines(&last, padding(origin)),
                " ".repeat(
                    padding(first_origin)
                        .saturating_sub(padding(origin).saturating_add(last_width))
                        .max(usize::from(item.layout.min_term_gap_columns))
                ),
                lines.next().unwrap_or_default()
            ));
            output.extend(lines.map(|line| {
                indent_lines(
                    line,
                    padding(compose_origin(
                        compose_origin(body_origin, layout.indent_columns),
                        layout.continuation_indent_columns,
                    )),
                )
            }));
            let mut result = Flow::default();
            result.gap(layout.spacing_before_lines);
            result.push_text(output.join("\n"));
            result.extend(self.block_flow(&item.description[1..], body_origin));
            return result;
        }
        let terms = terms
            .into_iter()
            .map(|term| indent_lines(&term, padding(origin)))
            .collect::<Vec<_>>()
            .join("\n");
        let mut result = Flow::text(terms);
        result.extend(self.block_flow(&item.description, body_origin));
        result
    }

    fn stacked_table_flow(&self, rows: &[mant_ir::TableRow], origin: i32) -> Flow {
        let mut output = Flow::default();
        for cell in rows.iter().flat_map(|row| &row.cells) {
            output.extend(self.block_flow(&cell.blocks, origin));
        }
        output
    }

    fn cell_text(&self, cell: &TableCell) -> String {
        self.render_blocks(&cell.blocks, 0).replace('\n', " ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::LayoutHint;

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
        assert!(mant_protocol::geometry::has_bounded_gap(&blocks));
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
