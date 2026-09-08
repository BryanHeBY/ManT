//! One source-aware block layout for plain and decorated text.
//! Decorators must preserve visible content and boundary whitespace.
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
        // Blocks are separated by a single blank line by default. An explicit
        // vertical-space node *sets* the gap before the next block rather than
        // adding to it, so `.sp` and blank input lines are not double-counted
        // against the default paragraph separation (which previously turned one
        // requested blank line into several). Leading and trailing gaps are
        // dropped so a section never opens or closes with blank lines.
        let mut output = String::new();
        // A definition term is preceding content too. Its first body block is
        // normally tight (Some(0)); a continuation of an inline paragraph uses
        // the normal block gap (Some(1)). Explicit space can override either.
        let mut has_content = leading_gap.is_some();
        let mut default_gap = leading_gap.unwrap_or(1);
        let mut pending_blank_lines: Option<usize> = None;
        for block in blocks {
            if let Block::VerticalSpace { lines, .. } = block {
                if has_content {
                    let requested = usize::from(*lines);
                    pending_blank_lines = Some(pending_blank_lines.unwrap_or(0).max(requested));
                }
                continue;
            }
            let Some(text) = self.render_block(block, base_indent) else {
                continue;
            };
            if has_content {
                let blank_lines = pending_blank_lines.unwrap_or(default_gap);
                output.push_str(&"\n".repeat(blank_lines + 1));
            }
            output.push_str(&text);
            has_content = true;
            default_gap = 1;
            pending_blank_lines = None;
        }
        output
    }

    fn render_block(&self, block: &Block, base_indent: i32) -> Option<String> {
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
                return Some(self.render_list(
                    *kind,
                    items,
                    *compact,
                    compose_origin(base_indent, layout.indent_columns),
                ));
            }
            Block::DefinitionList {
                items,
                compact,
                layout,
                ..
            } => {
                return Some(self.render_definitions(
                    items,
                    *compact,
                    compose_origin(base_indent, layout.indent_columns),
                ));
            }
            Block::Table { rows, layout, .. } => (
                super::super::table::table_rows(rows, |cell| self.cell_text(cell)).join("\n"),
                layout.indent_columns,
            ),
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
            Block::VerticalSpace { .. } => return None,
            Block::ThematicBreak { .. } => ("---".to_owned(), 0),
        };
        let value = value.trim_matches('\n');
        (!value.trim().is_empty())
            .then(|| indent_lines(value, padding(compose_origin(base_indent, layout_indent))))
    }

    fn render_list(
        &self,
        kind: ListKind,
        items: &[ListItem],
        compact: bool,
        base_indent: i32,
    ) -> String {
        items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let marker = match kind {
                    ListKind::Ordered { .. } => {
                        format!("{}. ", kind.ordinal(index).expect("ordered list ordinal"))
                    }
                    ListKind::Bullet => "- ".to_owned(),
                    ListKind::Plain => String::new(),
                };
                let body_origin = compose_origin(base_indent, coordinate(text_width(&marker)));
                let body = self.render_blocks(&item.blocks, body_origin);
                if marker.is_empty() {
                    return (!body.is_empty()).then_some(body);
                }
                let prefix = format!("{}{marker}", " ".repeat(padding(base_indent)));
                if let Some(Block::Paragraph { layout, .. }) = item.blocks.first()
                    && let Some(gap) =
                        marker_run_in_gap(base_indent, text_width(&marker), layout.indent_columns)
                    && let Some(rest) = body.strip_prefix(
                        &" ".repeat(padding(compose_origin(body_origin, layout.indent_columns))),
                    )
                {
                    Some(format!("{prefix}{}{rest}", " ".repeat(gap)))
                } else if body.is_empty() {
                    Some(prefix.trim_end().to_owned())
                } else {
                    Some(format!("{}\n{body}", prefix.trim_end()))
                }
            })
            .collect::<Vec<_>>()
            .join(if compact { "\n" } else { "\n\n" })
    }

    fn render_definitions(
        &self,
        items: &[DefinitionItem],
        compact: bool,
        base_indent: i32,
    ) -> String {
        let rendered = items
            .iter()
            .filter_map(|item| {
                let value = self.render_definition(item, base_indent);
                if value.is_empty() {
                    return None;
                }
                Some((value, item.layout.spacing_before_lines))
            })
            .collect::<Vec<_>>();

        let Some((first, rest)) = rendered.split_first() else {
            return String::new();
        };
        let mut output = first.0.clone();
        for (item, spacing_before_lines) in rest {
            let blank_lines = spacing_before_lines.unwrap_or(u16::from(!compact));
            output.push_str(&"\n".repeat(usize::from(blank_lines) + 1));
            output.push_str(item);
        }
        output
    }

    fn render_definition(&self, item: &DefinitionItem, origin: i32) -> String {
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
                    padding(compose_origin(body_origin, layout.indent_columns)),
                )
            }));
            let mut result = output.join("\n");
            result.push_str(&self.render_block_sequence(
                &item.description[1..],
                body_origin,
                Some(1),
            ));
            return result;
        }
        let terms = terms
            .into_iter()
            .map(|term| indent_lines(&term, padding(origin)))
            .collect::<Vec<_>>()
            .join("\n");
        let body = self.render_block_sequence(
            &item.description,
            body_origin,
            (!terms.is_empty()).then_some(0),
        );
        format!("{terms}{body}")
    }

    fn cell_text(&self, cell: &TableCell) -> String {
        self.render_blocks(&cell.blocks, 0).replace('\n', " ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::LayoutHint;

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
