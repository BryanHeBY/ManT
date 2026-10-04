//! Literal payload, row occupancy and continuation reset as one buffer.
use super::layout;
use mant_ir::{Block, Inline, SourceSpan};

pub(super) struct LiteralFlow {
    nodes: Vec<Inline>,
    source: Option<SourceSpan>,
    last_filled_line: Option<u32>,
    tight_boundary: bool,
    ordinary_continuation: bool,
    row_occupied: bool,
    trailing_vertical_row: TrailingRow,
    formatter_column: FormatterColumn,
    adopted_layout: Option<mant_ir::LayoutHint>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum FormatterColumn {
    Origin,
    Advanced,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TrailingRow {
    Ordinary,
    AssertedVertical,
}

impl LiteralFlow {
    pub(super) fn has_output(&self) -> bool {
        !self.nodes.is_empty()
    }

    /// Rehome the active paragraph vector without consuming its native unit.
    /// Field anchors retain their exact indices; no completed row is replayed.
    pub(super) fn adopt_active_output(
        &mut self,
        nodes: Vec<Inline>,
        source: Option<SourceSpan>,
        occupied: bool,
    ) {
        assert_eq!(self.nodes.len(), 0);
        self.nodes = nodes;
        self.source = source;
        self.row_occupied = occupied;
        self.formatter_column = if occupied {
            FormatterColumn::Advanced
        } else {
            FormatterColumn::Origin
        };
    }

    /// Continue a filled source word in the output owner of an already
    /// printed, still-open device row. Bd BODY post calls `term_newln()`, but
    /// NOBREAK can keep that row alive (`mdoc_term.c:1474`; term.c:250-253).
    pub(super) fn append_filled_fragment(
        &mut self,
        formatter: &mut crate::mandoc::formatter::FormatterState,
        source: Option<SourceSpan>,
        starts_indented_line: bool,
        continues_line: bool,
        ordinary_text: bool,
        append: impl FnOnce(&mut crate::mandoc::inline::InlineBuilder),
    ) {
        let source_line = source.map(|span| span.line);
        let crossed_source_line = self
            .last_filled_line
            .zip(source_line)
            .is_some_and(|(previous, current)| current > previous);
        let has_executed_predecessor =
            self.row_occupied || formatter.execution.has_formatter_cell();
        formatter.note_definition_source_line();
        self.with_inline_builder(formatter, |builder| {
            let boundary = super::paragraph::filled_fragment_boundary(
                builder,
                starts_indented_line && has_executed_predecessor,
                crossed_source_line,
            );
            super::paragraph::append_filled_fragment(
                builder,
                boundary,
                continues_line,
                ordinary_text,
                append,
            );
        });
        self.row_occupied = formatter.execution.has_formatter_cell()
            || formatter.execution.has_open_native_device_row();
        self.formatter_column = if self.row_occupied {
            FormatterColumn::Advanced
        } else {
            FormatterColumn::Origin
        };
        if source_line.is_some() {
            self.last_filled_line = source_line;
        }
    }

    pub(super) fn with_inline_builder<R>(
        &mut self,
        formatter: &mut crate::mandoc::formatter::FormatterState,
        operation: impl FnOnce(&mut crate::mandoc::inline::InlineBuilder) -> R,
    ) -> R {
        let result = formatter.with_output_builder(&mut self.nodes, operation);
        // Metadata can follow a projected delimiter, and NOBREAK can keep
        // device graph alive after the input buffer has been consumed. The
        // operation's actual row receipt determines occupancy in both cases.
        self.row_occupied = formatter.execution.has_formatter_cell()
            || formatter.execution.has_open_native_device_row();
        self.formatter_column = if self.row_occupied {
            FormatterColumn::Advanced
        } else {
            FormatterColumn::Origin
        };
        result
    }

    /// Execute against the live literal destination. All private word owners
    /// remain provisional until the native row consumer retires the buffer.
    pub(super) fn execute_fragment(
        &mut self,
        formatter: &mut crate::mandoc::formatter::FormatterState,
        source: Option<SourceSpan>,
        fallback: bool,
        finishes_row: bool,
        append: impl FnOnce(&mut crate::mandoc::inline::InlineBuilder),
    ) -> bool {
        let (continued, asserted) = crate::mandoc::inline::lower_no_fill_fragment_with_formatter(
            formatter,
            &mut self.nodes,
            fallback,
            finishes_row,
            append,
        );
        let occupied = formatter.execution.has_formatter_cell()
            || formatter.execution.has_open_native_device_row();
        self.row_occupied = occupied;
        self.formatter_column = if occupied {
            FormatterColumn::Advanced
        } else {
            FormatterColumn::Origin
        };
        self.tight_boundary = continued;
        if self.source.is_none() {
            self.source = source;
        }
        self.trailing_vertical_row = if asserted {
            TrailingRow::AssertedVertical
        } else {
            TrailingRow::Ordinary
        };
        occupied
    }

    pub(super) fn settle_native_row(
        &mut self,
        formatter: &mut crate::mandoc::formatter::FormatterState,
    ) {
        formatter
            .no_fill_inline
            .finish_row(&mut formatter.execution, &mut self.nodes);
        if crate::mandoc::inline::ends_with_executed_line_break(&self.nodes) {
            self.row_occupied = false;
            self.formatter_column = FormatterColumn::Origin;
        }
    }

    pub(super) fn insert_link_cursor(&mut self, marker: String) {
        self.nodes.push(Inline::anchor(marker));
    }

    pub(super) fn discard_link_cursor(&mut self, marker: &str) {
        super::super::man_links::remove_link_cursor(&mut self.nodes, marker);
    }

    pub(super) fn wrap_first_link(
        &mut self,
        target: &mant_ir::LinkTarget,
        marker: &str,
        started: &mut bool,
        skip_visible: &mut usize,
    ) -> bool {
        super::super::man_links::wrap_first_visible_inline(
            &mut self.nodes,
            target,
            marker,
            started,
            skip_visible,
        )
    }

    pub(super) const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            source: None,
            last_filled_line: None,
            tight_boundary: false,
            ordinary_continuation: false,
            row_occupied: false,
            trailing_vertical_row: TrailingRow::Ordinary,
            formatter_column: FormatterColumn::Origin,
            adopted_layout: None,
        }
    }

    pub(super) const fn starts_new_row(&self, starts_line: bool) -> bool {
        starts_line && self.row_occupied && !self.tight_boundary
    }

    pub(super) fn end_line(&mut self) {
        if self.row_occupied {
            self.nodes.push(Inline::line_break());
            self.row_occupied = false;
            self.formatter_column = FormatterColumn::Origin;
        }
        self.tight_boundary = false;
        self.ordinary_continuation = false;
    }

    pub(super) fn append(
        &mut self,
        mut nodes: Vec<Inline>,
        source: Option<SourceSpan>,
        continues_line: bool,
        starts_line: bool,
        occupies_row: bool,
    ) {
        if !nodes.is_empty() || occupies_row {
            self.trailing_vertical_row = TrailingRow::Ordinary;
        }
        if nodes.is_empty() && occupies_row {
            nodes.push(Inline::Text {
                value: String::new(),
            });
        }
        if self.starts_new_row(starts_line) {
            // A word-end `\p` at the physical line tail has already emitted
            // this row boundary. The following source row must consume that
            // boundary rather than add an empty line of its own.
            if !matches!(self.nodes.last(), Some(Inline::LineBreak { .. })) {
                self.nodes.push(Inline::line_break());
            }
            self.row_occupied = false;
            self.ordinary_continuation = false;
            self.formatter_column = FormatterColumn::Origin;
        }
        if self.ordinary_continuation && occupies_row {
            self.nodes.push(Inline::Text {
                value: " ".to_owned(),
            });
            self.ordinary_continuation = false;
        }
        let ends_formatter_row = nodes
            .iter()
            .rev()
            .find(|node| !matches!(node, Inline::Anchor { .. }))
            .is_some_and(|node| matches!(node, Inline::LineBreak { .. }));
        if let Some(last_break) = nodes
            .iter()
            .rposition(|node| matches!(node, Inline::LineBreak { .. }))
        {
            self.formatter_column =
                if mant_ir::has_printable_character(&nodes[last_break.saturating_add(1)..]) {
                    FormatterColumn::Advanced
                } else {
                    FormatterColumn::Origin
                };
        } else if mant_ir::has_printable_character(&nodes) {
            self.formatter_column = FormatterColumn::Advanced;
        }
        self.nodes.extend(nodes);
        if ends_formatter_row {
            self.row_occupied = false;
        } else {
            self.row_occupied |= occupies_row;
        }
        self.tight_boundary = continues_line;
        if self.source.is_none() {
            self.source = source;
        }
    }

    /// Model `TERMP_NOBREAK + term_flushln()` without exposing device margin
    /// geometry in the IR. Pending cells are committed, while the following
    /// formatter word remains on this visual row at an ordinary boundary.
    pub(super) fn no_break_flush(
        &mut self,
        formatter: &mut crate::mandoc::formatter::FormatterState,
    ) {
        // roff_term_pre_mc() invokes the same term_flushln() consumer in
        // filled and no-fill modes (roff_term.c:147-151). Its accepted
        // prefix/rejected suffix must consume this live literal owner.
        self.with_inline_builder(
            formatter,
            crate::mandoc::inline::InlineBuilder::no_break_flush,
        );
        formatter.no_fill_inline.retire_consumed_cell();
        // with_inline_builder already observed the consumed buffer's live
        // native row. Earlier accepted rows remain in nodes, but cannot
        // make the current row occupied after a marker pass ended it.
        self.ordinary_continuation = false;
    }

    pub(super) const fn has_formatter_column(&self) -> bool {
        matches!(self.formatter_column, FormatterColumn::Advanced)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub(super) fn adopt(
        &mut self,
        nodes: Vec<Inline>,
        source: Option<SourceSpan>,
        layout: mant_ir::LayoutHint,
    ) {
        assert_eq!(self.nodes.len(), 0);
        let last_line = nodes
            .iter()
            .rposition(|node| matches!(node, Inline::LineBreak { .. }))
            .map_or(nodes.as_slice(), |index| &nodes[index + 1..]);
        self.row_occupied = mant_ir::has_printable_character(last_line);
        self.formatter_column = if self.row_occupied {
            FormatterColumn::Advanced
        } else {
            FormatterColumn::Origin
        };
        self.nodes = nodes;
        self.source = source;
        self.last_filled_line = source.map(|span| span.line);
        self.adopted_layout = Some(layout);
    }

    pub(super) fn take(&mut self, indent: crate::mandoc::layout::SourceIndent) -> Vec<Block> {
        let mut previous = std::mem::replace(self, Self::new());
        let origins = crate::mandoc::inline::trailing_completed_row_origins(&previous.nodes);
        // The typed receipts retain their original order across this owner.
        // Empty TEXT content must not enter the bounded spacing-request plan.
        retire_completed_tail(&mut previous.nodes, origins.len());
        // An already closed formatter row is joined to the next block once.
        // This decision belongs to the producer's actual row receipt:
        // public literal LineBreak remains content for every consumer.
        // Empty TEXT is an occupied-row witness and prevents consuming a
        // distinct completed literal row (term.c:475-497).
        if !previous.row_occupied
            && crate::mandoc::inline::consume_one_row_ending(&mut previous.nodes)
            && origins.is_empty()
            && previous.trailing_vertical_row == TrailingRow::AssertedVertical
        {
            previous.nodes.push(Inline::Text {
                value: String::new(),
            });
        }
        crate::mandoc::inline::prepare_inline_output(&mut previous.nodes);
        let layout = previous.adopted_layout.unwrap_or_else(|| layout(indent));
        let mut blocks = origins::literal_blocks(previous.nodes, layout, previous.source);
        append_completed_rows(&mut blocks, &origins, layout);
        blocks
    }
}

/// Consecutive literal rows remain content in the preceding literal owner;
/// a layout request instead creates a separate bounded gap. A literal group
/// after that gap starts its own empty-row block, preserving execution order.
fn append_completed_rows(
    blocks: &mut Vec<Block>,
    origins: &[crate::mandoc::inline::CompletedRowOrigin],
    layout: mant_ir::LayoutHint,
) {
    use crate::mandoc::inline::CompletedRowOrigin;
    let mut index = 0;
    while index < origins.len() {
        let origin = origins[index];
        let rows = origins[index..]
            .iter()
            .take_while(|next| **next == origin)
            .count();
        match origin {
            CompletedRowOrigin::Layout => blocks.push(Block::VerticalSpace {
                lines: u16::try_from(rows).unwrap_or(u16::MAX),
                source: None,
            }),
            CompletedRowOrigin::LiteralText => {
                if let Some(Block::Preformatted { children, .. }) = blocks.last_mut() {
                    children.extend(std::iter::repeat_n(Inline::line_break(), rows));
                    children.push(Inline::Text {
                        value: String::new(),
                    });
                } else {
                    let mut children = vec![Inline::Text {
                        value: String::new(),
                    }];
                    children.extend(std::iter::repeat_n(Inline::line_break(), rows - 1));
                    if rows > 1 {
                        // Every completed empty TEXT row is literal content
                        // (term_vspace, term.c:489-497). A typed delimiter
                        // closes the preceding row; keep the last empty row
                        // witness distinct from a block's ordinary close.
                        children.push(Inline::Text {
                            value: String::new(),
                        });
                    }
                    blocks.push(Block::Preformatted {
                        inline_layout: mant_ir::InlineLayout::default(),
                        children,
                        language: None,
                        layout,
                        source: None,
                    });
                }
            }
        }
        index += rows;
    }
}

/// Each actual `term_vspace()` row transfers exactly once. Scan the tail in
/// one reverse pass rather than searching past retained identities once for
/// every row. Authored spaces remain cells; only empty occupancy witnesses
/// associated with this completed tail can be retired.
fn retire_completed_tail(nodes: &mut Vec<Inline>, completed_rows: usize) {
    let mut tail = CompletedTail {
        remaining: completed_rows,
        active: completed_rows > 0,
        #[cfg(test)]
        visits: 0,
    };
    tail.retire(nodes);
}

struct CompletedTail {
    remaining: usize,
    active: bool,
    #[cfg(test)]
    visits: usize,
}

impl CompletedTail {
    fn retire(&mut self, nodes: &mut Vec<Inline>) {
        let mut retained = Vec::with_capacity(nodes.len());
        for mut node in nodes.drain(..).rev() {
            #[cfg(test)]
            {
                self.visits += 1;
            }
            if self.active {
                match &mut node {
                    Inline::Anchor { .. } => {}
                    Inline::Text { value } if value.is_empty() => continue,
                    Inline::LineBreak { .. } if self.remaining > 0 => {
                        self.remaining -= 1;
                        continue;
                    }
                    Inline::Strong { children }
                    | Inline::Emphasis { children }
                    | Inline::Link { children, .. } => self.retire(children),
                    _ => self.active = false,
                }
            }
            retained.push(node);
        }
        retained.reverse();
        *nodes = retained;
    }
}

mod origins;
#[cfg(test)]
mod tests;
