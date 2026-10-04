//! List markers, definition heads, and their shared content/anchor ownership.
use super::super::inline::{shifted_reference_marks, spans_scalars};
use super::super::{
    Block, ListKind, LogicalLine, Span, StyledInlineLine, WrapMode, inline_anchor_rows,
    shifted_links, spans_width, theme,
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
                ListKind::Dash => "- ".to_owned(),
                ListKind::Ordered { .. } => {
                    format!("{}. ", kind.ordinal(index).expect("ordered list ordinal"))
                }
                ListKind::Plain => String::new(),
            };
            let has_marker = !marker.is_empty();
            let marker_width = mant_ir::geometry::text_width(&marker);
            if has_marker
                && let Some(Block::Paragraph {
                    inline_layout,
                    layout,
                    ..
                }) = item.blocks.first()
                && let Some(gap) = marker_run_in_gap(
                    indent,
                    marker_width,
                    compose_origin(layout.indent_columns, inline_layout.row_indent(0)),
                )
            {
                self.run_in_list_paragraph(&item.blocks[0], marker, indent, marker_width, gap);
                self.blocks(
                    &item.blocks[1..],
                    compose_origin(indent, coordinate(marker_width)),
                );
            } else {
                if has_marker {
                    self.push(LogicalLine::plain(
                        padding(indent),
                        marker,
                        theme::style(theme::StyleRole::ListMarker),
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

    fn run_in_list_paragraph(
        &mut self,
        paragraph: &Block,
        marker: String,
        indent: i32,
        marker_width: usize,
        gap: usize,
    ) {
        let Block::Paragraph {
            children,
            inline_layout,
            layout,
            ..
        } = paragraph
        else {
            unreachable!("run-in admission selects a paragraph");
        };
        self.spacing(layout.spacing_before_lines);
        let content_indent = compose_origin(
            compose_origin(indent, coordinate(marker_width)),
            layout.indent_columns,
        );
        let continuation_indent =
            compose_origin(content_indent, layout.continuation_indent_columns);
        let mut inline_lines = self.styled_inlines(
            mant_ir::InlineContentRef {
                content: children,
                layout: inline_layout,
            },
            theme::style(theme::StyleRole::Text),
        );
        let mut deferred_targets = Self::trim_paragraph_tail(&mut inline_lines);
        for (id, row) in inline_anchor_rows(children) {
            if row >= inline_lines.len() {
                deferred_targets.push(id);
            } else {
                self.anchors.entry(id).or_insert(self.lines.len() + row);
            }
        }
        let first = inline_lines
            .first_mut()
            .map_or_else(StyledInlineLine::default, std::mem::take);
        let mut spans = vec![Span::styled(
            marker,
            theme::style(theme::StyleRole::ListMarker),
        )];
        spans.push(Span::raw(" ".repeat(gap)));
        spans.extend(first.spans);
        let origins =
            mant_ir::resolve_row_origins(content_indent, continuation_indent, first.indent_columns);
        let base_gap = marker_run_in_gap(indent, marker_width, layout.indent_columns).unwrap_or(0);
        let mut logical =
            LogicalLine::hanging(padding(indent), padding(origins.continuation_origin), spans)
                .with_links(shifted_links(first.links, marker_width.saturating_add(gap)))
                .with_reference_marks(shifted_reference_marks(
                    first.reference_marks,
                    marker_width.saturating_add(gap),
                ));
        logical.continuation_layout_padding = logical
            .continuation_indent
            .saturating_sub(padding(continuation_indent));
        if gap > base_gap {
            logical
                .layout_scalars
                .push(marker_width.saturating_add(base_gap)..marker_width.saturating_add(gap));
        }
        self.push(logical);
        for line in inline_lines.into_iter().skip(1) {
            self.push(
                LogicalLine::row_geometry(
                    continuation_indent,
                    continuation_indent,
                    line.indent_columns,
                    line.spans,
                )
                .with_links(line.links)
                .with_reference_marks(line.reference_marks),
            );
        }
        self.defer_anchors(deferred_targets);
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
            if item.layout.inline_term() {
                self.inline_definition(item, indent);
            } else {
                for term in &item.terms {
                    self.inline_lines_with_surface(
                        term.inline_content(),
                        indent,
                        theme::style(theme::StyleRole::Text),
                        super::super::LineSurface::Normal,
                    );
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
            let lines =
                self.styled_inlines(term.inline_content(), theme::style(theme::StyleRole::Text));
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
        self.register_definition_head_targets(head_targets, last_target_row);
        let last = head_lines.pop().unwrap_or_default();
        for line in head_lines {
            self.push(
                LogicalLine::row_geometry(indent, indent, line.indent_columns, line.spans)
                    .with_links(line.links)
                    .with_reference_marks(line.reference_marks),
            );
        }
        if let Some((content, layout)) = item.inline_description_content() {
            let children = content.content;
            let mut description_lines =
                self.styled_inlines(content, theme::style(theme::StyleRole::Text));
            let literal_inline =
                matches!(item.description.first(), Some(Block::Preformatted { .. }));
            let mut deferred_targets = if literal_inline {
                Vec::new()
            } else {
                Self::trim_paragraph_tail(&mut description_lines)
            };
            for (id, row) in inline_anchor_rows(children) {
                if row >= description_lines.len() {
                    deferred_targets.push(id);
                } else {
                    self.anchors.entry(id).or_insert(self.lines.len() + row);
                }
            }
            // A run-in literal keeps its authored spacing: the shared row and
            // its continuations wrap as characters, never as words.
            let wrap_mode = if literal_inline {
                WrapMode::Character
            } else {
                WrapMode::Word
            };
            let first = description_lines
                .first_mut()
                .map_or_else(StyledInlineLine::default, std::mem::take);
            let (logical, continuation_indent) =
                Self::shared_definition_line(item, indent, last, first, layout, wrap_mode);
            self.push(logical);
            for line in description_lines.into_iter().skip(1) {
                self.push(
                    LogicalLine::row_geometry(
                        continuation_indent,
                        continuation_indent,
                        line.indent_columns,
                        line.spans,
                    )
                    .wrap_mode(wrap_mode)
                    .with_links(line.links)
                    .with_reference_marks(line.reference_marks),
                );
            }
            self.defer_anchors(deferred_targets);
            self.blocks(&item.description[1..], block_origin);
        } else {
            self.push(
                LogicalLine::row_geometry(indent, indent, last.indent_columns, last.spans)
                    .with_links(last.links)
                    .with_reference_marks(last.reference_marks),
            );
            self.blocks(&item.description, block_origin);
        }
    }

    fn shared_definition_line(
        item: &DefinitionItem,
        indent: i32,
        head: StyledInlineLine,
        body: StyledInlineLine,
        layout: &mant_ir::LayoutHint,
        wrap_mode: WrapMode,
    ) -> (LogicalLine, i32) {
        let term_origin = compose_origin(indent, head.indent_columns);
        let mut term_spans = head.spans;
        let mut term_links = head.links;
        let mut term_marks = head.reference_marks;
        // Measure the final visible head's graphemes at the shared row.
        let term_width = spans_width(&term_spans);
        let block_origin = compose_origin(indent, item.layout.body_indent_columns);
        let first_indent = compose_origin(block_origin, layout.indent_columns);
        let continuation_indent = compose_origin(first_indent, layout.continuation_indent_columns);
        let gap = mant_ir::geometry::definition_body_gap(
            &item.layout,
            term_origin,
            term_width,
            compose_origin(first_indent, body.indent_columns),
        );
        term_spans.push(Span::raw(" ".repeat(gap)));
        let description_scalar_offset = spans_scalars(&term_spans);
        term_links.extend(shifted_links(body.links, description_scalar_offset));
        term_marks.extend(shifted_reference_marks(
            body.reference_marks,
            description_scalar_offset,
        ));
        term_spans.extend(body.spans);
        let origins =
            mant_ir::resolve_row_origins(first_indent, continuation_indent, body.indent_columns);
        let base_gap = mant_ir::geometry::definition_body_gap(
            &item.layout,
            term_origin,
            term_width,
            first_indent,
        );
        let mut logical = LogicalLine::hanging(
            padding(term_origin),
            padding(origins.continuation_origin),
            term_spans,
        )
        .wrap_mode(wrap_mode)
        .with_links(term_links)
        .with_reference_marks(term_marks);
        logical.layout_padding = logical.indent.saturating_sub(padding(indent));
        logical.continuation_layout_padding = logical
            .continuation_indent
            .saturating_sub(padding(continuation_indent));
        if gap > base_gap {
            let seam_start = description_scalar_offset.saturating_sub(gap);
            logical
                .layout_scalars
                .push(seam_start.saturating_add(base_gap)..seam_start.saturating_add(gap));
        }
        (logical, continuation_indent)
    }

    fn register_definition_head_targets(
        &mut self,
        targets: Vec<(String, usize)>,
        last_target_row: usize,
    ) {
        for (id, row) in targets {
            self.anchors
                .entry(id)
                .or_insert(self.lines.len() + row.min(last_target_row));
        }
    }
}
