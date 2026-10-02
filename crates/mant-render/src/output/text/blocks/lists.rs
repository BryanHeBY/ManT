//! List labels and definition bodies compose into the caller's content flow.
use super::{
    Block, BlockRenderer, DefinitionItem, Flow, LayoutText, ListItem, ListKind, TextRole,
    compose_origin, coordinate, marker_run_in_gap, padding, text_width,
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
            if let Some(first @ Block::Paragraph { layout, .. }) = item.blocks.first()
                && let Some(gap) =
                    marker_run_in_gap(base_indent, text_width(&marker), layout.indent_columns)
            {
                // The first paragraph's boundary precedes the whole item,
                // not the text after its marker. Keep it out of string
                // prefix tests and preserve subsequent hard-line origins.
                output.gap(layout.spacing_before_lines);
                let body = self.render_block(first, body_origin).finish_layout(false);
                let indent =
                    " ".repeat(padding(compose_origin(body_origin, layout.indent_columns)));
                let rest = body.strip_prefix(&indent);
                output.push_text(rest.prefixed(&format!("{prefix}{}", " ".repeat(gap))));
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
                let rows = self.inline_rows(term, TextRole::DefinitionTerm);
                // A term that only breaks rows still owns those rows (an
                // empty `.It` operand's field owns its blank lines).
                (rows.len() > 1 || rows.iter().any(|(row, _)| !row.is_empty())).then_some(rows)
            })
            .collect::<Vec<_>>();
        // The relation records the word boundary and first-row alignment.
        // Layout hints resolve columns without changing accepted adjacency.
        if !matches!(
            item.layout.head_body_relation,
            mant_ir::HeadBodyRelation::Separate
        ) && let Some((children, layout)) = item.inline_description()
            && let Some(last_rows) = terms.pop()
        {
            let term_origin = compose_origin(
                origin,
                i32::from(last_rows.last().map_or(0, |(_, indent)| *indent)),
            );
            let last = LayoutText::join(
                last_rows.into_iter().map(|(row, row_indent)| {
                    row.indented(padding(compose_origin(origin, i32::from(row_indent))))
                }),
                "\n",
            );
            let last_width = mant_ir::geometry::definition_run_in_width(&item.terms).unwrap_or(0);
            let mut lines = self.inline_rows(children, TextRole::Body).into_iter();
            let first_line = lines.next().unwrap_or_default();
            let preferred_body_origin = compose_origin(
                compose_origin(body_origin, layout.indent_columns),
                i32::from(first_line.1),
            );
            let mut output = definition_term_rows(terms, origin);
            let mut joined = last;
            let gap = mant_ir::geometry::definition_body_gap(
                &item.layout,
                term_origin,
                last_width,
                preferred_body_origin,
            );
            joined.push_plain(&" ".repeat(gap));
            joined.append(&first_line.0);
            output.push(joined);
            output.extend(lines.map(|(line, row_indent)| {
                line.indented(padding(compose_origin(
                    compose_origin(
                        compose_origin(body_origin, layout.indent_columns),
                        layout.continuation_indent_columns,
                    ),
                    i32::from(row_indent),
                )))
            }));
            let mut result = Flow::default();
            result.gap(layout.spacing_before_lines);
            result.push_text(LayoutText::join(output, "\n"));
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

fn definition_term_rows(terms: Vec<Vec<(LayoutText, u16)>>, origin: i32) -> Vec<LayoutText> {
    terms
        .into_iter()
        .flat_map(|term| {
            term.into_iter().map(|(row, row_indent)| {
                row.indented(padding(compose_origin(origin, i32::from(row_indent))))
            })
        })
        .collect()
}
