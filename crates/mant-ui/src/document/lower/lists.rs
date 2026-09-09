//! List markers, definition heads, and their shared content/anchor ownership.
use super::super::inline::shifted_reference_marks;
use super::super::{
    Block, ListKind, LogicalLine, Span, Style, StyledInlineLine, inline_anchor_rows, shifted_links,
    spans_width, theme,
};
use super::DocumentBuilder;
use mant_ir::geometry::{compose_origin, coordinate, marker_run_in_gap, padding};
use mant_ir::{DefinitionItem, ListItem};

impl DocumentBuilder<'_> {
    pub(super) fn list(&mut self, kind: ListKind, compact: bool, items: &[ListItem], indent: i32) {
        for (index, item) in items.iter().enumerate() {
            self.spacing(
                item.layout
                    .spacing_before_lines
                    .unwrap_or(u16::from(index > 0 && !compact)),
            );
            let item_start = self.lines.len();
            let marker = match kind {
                ListKind::Bullet => "• ".to_owned(),
                ListKind::Ordered { .. } => {
                    format!("{}. ", kind.ordinal(index).expect("ordered list ordinal"))
                }
                ListKind::Plain => String::new(),
            };
            let has_marker = !marker.is_empty();
            let marker_width = mant_ir::geometry::text_width(&marker);
            if has_marker
                && let Some(Block::Paragraph {
                    children, layout, ..
                }) = item.blocks.first()
                && let Some(gap) = marker_run_in_gap(indent, marker_width, layout.indent_columns)
            {
                self.spacing(layout.spacing_before_lines);
                for (id, row) in inline_anchor_rows(children) {
                    self.anchors.entry(id).or_insert(self.lines.len() + row);
                }
                let content_indent = compose_origin(
                    compose_origin(indent, coordinate(marker_width)),
                    layout.indent_columns,
                );
                let continuation_indent =
                    compose_origin(content_indent, layout.continuation_indent_columns);
                let mut inline_lines =
                    self.styled_inlines(children, Style::default().fg(theme::TEXT));
                let first = inline_lines
                    .first_mut()
                    .map_or_else(StyledInlineLine::default, std::mem::take);
                let mut spans = vec![Span::styled(marker, Style::default().fg(theme::HEADING))];
                spans.push(Span::raw(" ".repeat(gap)));
                spans.extend(first.spans);
                self.push(
                    LogicalLine::hanging(padding(indent), padding(continuation_indent), spans)
                        .with_links(shifted_links(first.links, marker_width.saturating_add(gap)))
                        .with_reference_marks(shifted_reference_marks(
                            first.reference_marks,
                            marker_width.saturating_add(gap),
                        )),
                );
                for line in inline_lines.into_iter().skip(1) {
                    self.push(
                        LogicalLine::hanging(
                            padding(continuation_indent),
                            padding(continuation_indent),
                            line.spans,
                        )
                        .with_links(line.links)
                        .with_reference_marks(line.reference_marks),
                    );
                }
                self.blocks(
                    &item.blocks[1..],
                    compose_origin(indent, coordinate(marker_width)),
                );
            } else {
                if has_marker {
                    self.push(LogicalLine::plain(
                        padding(indent),
                        marker,
                        Style::default().fg(theme::HEADING),
                    ));
                }
                self.blocks(
                    &item.blocks,
                    compose_origin(indent, coordinate(marker_width)),
                );
            }
            if let Some(facts) = &item.entry {
                // Leading spacing is presentation, not the semantic landing row.
                let first_content = self.lines[item_start..]
                    .iter()
                    .position(|line| !line.spans.is_empty() || line.table_row.is_some())
                    .map_or(item_start, |offset| item_start + offset);
                self.anchors.insert(facts.id.to_string(), first_content);
            }
        }
    }

    pub(super) fn definitions(&mut self, items: &[DefinitionItem], compact: bool, indent: i32) {
        for (index, item) in items.iter().enumerate() {
            let spacing = item
                .layout
                .spacing_before_lines
                .unwrap_or(u16::from(index > 0 && !compact));
            self.spacing(spacing);
            if let Some(identity) = &item.entry {
                self.anchors
                    .insert(identity.id.to_string(), self.lines.len());
            }
            if item.layout.inline_term {
                self.inline_definition(item, indent);
            } else {
                for term in &item.terms {
                    self.inline_lines(term, indent, Style::default().fg(theme::TEXT));
                }
                self.blocks(
                    &item.description,
                    compose_origin(indent, item.layout.body_indent_columns),
                );
            }
        }
    }

    fn definition_head(
        &self,
        item: &mant_ir::DefinitionItem,
    ) -> (Vec<StyledInlineLine>, Vec<(String, usize)>) {
        let mut head_lines = Vec::new();
        let mut head_targets = Vec::new();
        for term in &item.terms {
            for (id, row) in inline_anchor_rows(term) {
                head_targets.push((id, head_lines.len() + row));
            }
            let lines = self.styled_inlines(term, Style::default().fg(theme::TEXT));
            if lines.len() != 1 || !lines[0].spans.is_empty() {
                head_lines.extend(lines);
            } else {
                head_targets.extend(
                    lines[0]
                        .reference_marks
                        .iter()
                        .map(|mark| (mark.id.to_string(), head_lines.len())),
                );
            }
        }
        (head_lines, head_targets)
    }

    fn inline_definition(&mut self, item: &mant_ir::DefinitionItem, indent: i32) {
        let (mut head_lines, head_targets) = self.definition_head(item);
        let block_origin = compose_origin(indent, item.layout.body_indent_columns);
        if head_lines.is_empty() {
            self.defer_anchors(head_targets.into_iter().map(|(id, _)| id));
            self.blocks(&item.description, block_origin);
            return;
        }
        // A trailing zero-width root shares the last head/body row when that
        // row runs in. Its provisional slot is not a new visible line.
        let last_target_row = if item.inline_description().is_some() {
            head_lines.len().saturating_sub(1)
        } else {
            head_lines.len()
        };
        for (id, row) in head_targets {
            self.anchors
                .entry(id)
                .or_insert(self.lines.len() + row.min(last_target_row));
        }
        let last = head_lines.pop().unwrap_or_default();
        for line in head_lines {
            self.push(
                LogicalLine::hanging(padding(indent), padding(indent), line.spans)
                    .with_links(line.links)
                    .with_reference_marks(line.reference_marks),
            );
        }
        let mut term_spans = last.spans;
        let mut term_links = last.links;
        let mut term_marks = last.reference_marks;
        let term_width = mant_ir::geometry::definition_run_in_width(&item.terms).unwrap_or(0);
        if let Some((children, layout)) = item.inline_description() {
            for (id, row) in inline_anchor_rows(children) {
                self.anchors.entry(id).or_insert(self.lines.len() + row);
            }
            let first_indent = compose_origin(block_origin, layout.indent_columns);
            let continuation_indent =
                compose_origin(first_indent, layout.continuation_indent_columns);
            let description_indent = first_indent.max(compose_origin(
                indent,
                coordinate(
                    term_width.saturating_add(usize::from(item.layout.min_term_gap_columns)),
                ),
            ));
            term_spans.push(Span::raw(
                " ".repeat(
                    padding(description_indent)
                        .saturating_sub(padding(indent).saturating_add(term_width))
                        .max(usize::from(item.layout.min_term_gap_columns)),
                ),
            ));
            let mut description_lines =
                self.styled_inlines(children, Style::default().fg(theme::TEXT));
            let first = description_lines
                .first_mut()
                .map_or_else(StyledInlineLine::default, std::mem::take);
            let description_offset = spans_width(&term_spans);
            let description_scalar_offset = term_spans
                .iter()
                .map(|span| span.content.chars().count())
                .sum();
            term_links.extend(shifted_links(first.links, description_offset));
            term_marks.extend(shifted_reference_marks(
                first.reference_marks,
                description_scalar_offset,
            ));
            term_spans.extend(first.spans);
            self.push(
                LogicalLine::hanging(padding(indent), padding(continuation_indent), term_spans)
                    .with_links(term_links)
                    .with_reference_marks(term_marks),
            );
            for line in description_lines.into_iter().skip(1) {
                self.push(
                    LogicalLine::hanging(
                        padding(continuation_indent),
                        padding(continuation_indent),
                        line.spans,
                    )
                    .with_links(line.links)
                    .with_reference_marks(line.reference_marks),
                );
            }
            self.blocks(&item.description[1..], block_origin);
        } else {
            self.push(
                LogicalLine::hanging(padding(indent), padding(indent), term_spans)
                    .with_links(term_links)
                    .with_reference_marks(term_marks),
            );
            self.blocks(&item.description, block_origin);
        }
    }
}
