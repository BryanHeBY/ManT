//! List labels and definition bodies compose into the caller's content flow.
use super::{
    Block, BlockRenderer, DefinitionItem, Flow, ListItem, ListKind, TextRole, compose_origin,
    coordinate, indent_lines, marker_run_in_gap, padding, text_width,
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
            .map(|term| self.inline_text(term, TextRole::DefinitionTerm))
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();
        if let Some((children, layout)) = item.inline_description()
            && let Some(last) = terms.pop()
        {
            let last_width = mant_ir::geometry::definition_run_in_width(&item.terms).unwrap_or(0);
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
}
