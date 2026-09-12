//! Literal payload, row occupancy and continuation reset as one buffer.
use super::layout;
use mant_ir::{Block, Inline, SourceSpan};

pub(super) struct LiteralFlow {
    nodes: Vec<Inline>,
    source: Option<SourceSpan>,
    tight_boundary: bool,
    ordinary_continuation: bool,
    row_occupied: bool,
}

impl LiteralFlow {
    pub(super) const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            source: None,
            tight_boundary: false,
            ordinary_continuation: false,
            row_occupied: false,
        }
    }

    pub(super) const fn starts_new_row(&self, starts_line: bool) -> bool {
        starts_line && self.row_occupied && !self.tight_boundary
    }

    pub(super) fn end_line(&mut self) {
        if self.row_occupied {
            self.nodes.push(Inline::LineBreak);
            self.row_occupied = false;
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
        if nodes.is_empty() && occupies_row {
            nodes.push(Inline::Text {
                value: String::new(),
            });
        }
        if self.starts_new_row(starts_line) {
            // A word-end `\p` at the physical line tail has already emitted
            // this row boundary. The following source row must consume that
            // boundary rather than add an empty line of its own.
            if !matches!(self.nodes.last(), Some(Inline::LineBreak)) {
                self.nodes.push(Inline::LineBreak);
            }
            self.row_occupied = false;
        }
        if self.ordinary_continuation && occupies_row {
            self.nodes.push(Inline::Text {
                value: " ".to_owned(),
            });
            self.ordinary_continuation = false;
        }
        self.nodes.extend(nodes);
        self.row_occupied |= occupies_row;
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
        if self.row_occupied {
            self.tight_boundary = true;
            self.ordinary_continuation = true;
        }
    }

    pub(super) fn take(&mut self, indent: crate::mandoc::layout::SourceIndent) -> Option<Block> {
        let mut previous = std::mem::replace(self, Self::new());
        // A formatter-only word closes a row without occupying the next one.
        // Actual empty rows end with an explicit empty text sentinel.
        if !previous.row_occupied && matches!(previous.nodes.last(), Some(Inline::LineBreak)) {
            previous.nodes.pop();
        }
        (!previous.nodes.is_empty()).then(|| Block::Preformatted {
            children: previous.nodes,
            language: None,
            layout: layout(indent),
            source: previous.source,
        })
    }
}
