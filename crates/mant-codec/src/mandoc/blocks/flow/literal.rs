//! Literal payload, row occupancy and continuation reset as one buffer.
use super::layout;
use mant_ir::{Block, Inline, SourceSpan};

pub(super) struct LiteralFlow {
    nodes: Vec<Inline>,
    source: Option<SourceSpan>,
    tight_boundary: bool,
    row_occupied: bool,
}

impl LiteralFlow {
    pub(super) const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            source: None,
            tight_boundary: false,
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
            self.nodes.push(Inline::LineBreak);
            self.row_occupied = false;
        }
        self.nodes.extend(nodes);
        self.row_occupied |= occupies_row;
        self.tight_boundary = continues_line;
        if self.source.is_none() {
            self.source = source;
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
