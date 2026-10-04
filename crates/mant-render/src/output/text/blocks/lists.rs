//! List labels and definition bodies compose into the caller's content flow.
use super::{
    Block, BlockRenderer, DefinitionItem, Flow, LayoutText, ListItem, ListKind, ParagraphTail,
    TextRole, compose_origin, coordinate, marker_run_in_gap, padding, resolve_row_origins,
    text_width,
};

impl BlockRenderer<'_> {
    pub(super) fn render_list(
        &self,
        kind: ListKind,
        items: &[ListItem],
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
            let marker = match kind {
                ListKind::Ordered { .. } => {
                    format!("{}. ", kind.ordinal(index).expect("ordered list ordinal"))
                }
                ListKind::Bullet => "• ".to_owned(),
                ListKind::Dash => "- ".to_owned(),
                ListKind::Plain => String::new(),
            };
            let body_origin = compose_origin(base_indent, coordinate(text_width(&marker)));
            if marker.is_empty() {
                output.extend(self.block_flow(&item.blocks, body_origin));
                continue;
            }
            let prefix = format!("{}{marker}", " ".repeat(padding(base_indent)));
            if let Some(
                first @ Block::Paragraph {
                    layout,
                    inline_layout,
                    ..
                },
            ) = item.blocks.first()
                && let Some(gap) = marker_run_in_gap(
                    base_indent,
                    text_width(&marker),
                    compose_origin(layout.indent_columns, inline_layout.row_indent(0)),
                )
            {
                // The first paragraph's boundary precedes the whole item,
                // not the text after its marker. Keep it out of string
                // prefix tests and preserve subsequent hard-line origins.
                output.gap(layout.spacing_before_lines);
                let body = self.render_block(first, body_origin);
                let indent = " ".repeat(padding(
                    resolve_row_origins(
                        compose_origin(body_origin, layout.indent_columns),
                        compose_origin(
                            compose_origin(body_origin, layout.indent_columns),
                            layout.continuation_indent_columns,
                        ),
                        inline_layout.row_indent(0),
                    )
                    .first_visual_origin,
                ));
                output.extend(
                    body.prefix_first_row(&indent, &format!("{prefix}{}", " ".repeat(gap))),
                );
                output.extend(self.block_flow(&item.blocks[1..], body_origin));
            } else {
                output.push_text(prefix.trim_end().into());
                output.extend(self.block_flow(&item.blocks, body_origin));
            }
        }
        output
    }

    pub(super) fn render_definitions(
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
            .filter_map(|term| {
                let rows = self.inline_rows(term.inline_content(), TextRole::DefinitionTerm);
                // A term that only breaks rows still owns those rows (an
                // empty `.It` operand's field owns its blank lines).
                (rows.len() > 1 || rows.iter().any(|(row, _)| !row.is_empty())).then_some(rows)
            })
            .collect::<Vec<_>>();
        // The item records shared/separate rows and the word boundary.
        // Independent layout hints resolve columns without changing adjacency.
        if !matches!(item.head_body_relation, mant_ir::HeadBodyRelation::Separate)
            && let Some((content, layout)) = item.inline_description_content()
            && let Some(last_rows) = terms.pop()
        {
            let term_origin =
                compose_origin(origin, last_rows.last().map_or(0, |(_, indent)| *indent));
            let last = LayoutText::join(
                last_rows.into_iter().map(|(row, row_indent)| {
                    row.indented(padding(
                        resolve_row_origins(origin, origin, row_indent).first_visual_origin,
                    ))
                }),
                "\n",
            );
            let last_width = mant_ir::geometry::definition_run_in_width(&item.terms).unwrap_or(0);
            let mut lines = self.inline_rows(content, TextRole::Body);
            let completed_empty_tail =
                matches!(item.description.first(), Some(Block::Paragraph { .. }))
                    && Self::close_paragraph_rows(&mut lines, ParagraphTail::BlockBoundary);
            let mut lines = lines.into_iter();
            let first_line = lines.next().unwrap_or_default();
            let first_body_origin = compose_origin(body_origin, layout.indent_columns);
            let continued_body_origin =
                compose_origin(first_body_origin, layout.continuation_indent_columns);
            let preferred_body_origin =
                resolve_row_origins(first_body_origin, continued_body_origin, first_line.1)
                    .first_visual_origin;
            let mut output = definition_term_rows(terms, origin);
            let mut joined = last;
            let gap = mant_ir::geometry::definition_body_gap(
                item.head_body_relation,
                &item.layout,
                term_origin,
                last_width,
                preferred_body_origin,
            );
            joined.push_plain(&" ".repeat(gap));
            joined.append(&first_line.0);
            output.push(joined);
            output.extend(lines.map(|(line, row_indent)| {
                line.indented(padding(
                    resolve_row_origins(continued_body_origin, continued_body_origin, row_indent)
                        .first_visual_origin,
                ))
            }));
            let mut result = Flow::default();
            result.gap(layout.spacing_before_lines);
            let value = LayoutText::join(output, "\n");
            result.extend(if completed_empty_tail {
                Flow::completed_text(value)
            } else {
                Flow::text(value)
            });
            result.extend(self.block_flow(&item.description[1..], body_origin));
            return result;
        }
        // Rows keep their request-relative indents (a cleared-BRIND request
        // moved the upstream offset, roff_term.c:73-75).
        let rows = definition_term_rows(terms, origin);
        let mut result = Flow::text(LayoutText::join(rows, "\n"));
        result.extend(self.block_flow(&item.description, body_origin));
        result
    }
}

fn definition_term_rows(terms: Vec<Vec<(LayoutText, i32)>>, origin: i32) -> Vec<LayoutText> {
    terms
        .into_iter()
        .flat_map(|term| {
            term.into_iter().map(|(row, row_indent)| {
                row.indented(padding(
                    resolve_row_origins(origin, origin, row_indent).first_visual_origin,
                ))
            })
        })
        .collect()
}
