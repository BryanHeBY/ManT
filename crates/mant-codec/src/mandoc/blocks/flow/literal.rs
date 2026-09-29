//! Literal payload, row occupancy and continuation reset as one buffer.
use super::layout;
use mant_ir::{Block, Inline, SourceSpan};

pub(super) struct LiteralFlow {
    nodes: Vec<Inline>,
    source: Option<SourceSpan>,
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
    pub(super) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub(super) fn wrap_first_link(
        &mut self,
        target: &mant_ir::LinkTarget,
        start: usize,
        skip_visible: &mut usize,
    ) -> bool {
        super::super::man_links::wrap_first_visible_inline(
            &mut self.nodes,
            target,
            start,
            skip_visible,
        )
    }

    pub(super) const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            source: None,
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

    pub(super) fn mark_vertical_row(&mut self) {
        self.trailing_vertical_row = TrailingRow::AssertedVertical;
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
    pub(super) fn no_break_flush(&mut self, nodes: Vec<Inline>) {
        let committed_a_cell = mant_ir::has_printable_character(&nodes);
        self.nodes.extend(nodes);
        self.row_occupied |= committed_a_cell;
        // The caller invokes this only for an active formatter cell. `.mc`
        // releases NOSPACE but preserves an independently active NONEWLINE.
        // LiteralFlow represents that as an ordinary continuation across a
        // still-tight physical row boundary.
        self.ordinary_continuation = self.tight_boundary;
        self.formatter_column = FormatterColumn::Origin;
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
        debug_assert!(self.nodes.is_empty());
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
        self.adopted_layout = Some(layout);
    }

    pub(super) fn take(&mut self, indent: crate::mandoc::layout::SourceIndent) -> Option<Block> {
        let mut previous = std::mem::replace(self, Self::new());
        // A formatter-only word closes a row without occupying the next one.
        // A visited empty TEXT asserts one empty row at this edge. Encode that
        // row as an empty cell, since an IR-only terminal LineBreak would add
        // another blank line when this block is followed by a new owner.
        if !previous.row_occupied && matches!(previous.nodes.last(), Some(Inline::LineBreak { .. }))
        {
            previous.nodes.pop();
            if previous.trailing_vertical_row == TrailingRow::AssertedVertical {
                previous.nodes.push(Inline::Text {
                    value: String::new(),
                });
            }
        }
        (!previous.nodes.is_empty()).then(|| Block::Preformatted {
            children: previous.nodes,
            language: None,
            layout: previous.adopted_layout.unwrap_or_else(|| layout(indent)),
            source: previous.source,
        })
    }
}
