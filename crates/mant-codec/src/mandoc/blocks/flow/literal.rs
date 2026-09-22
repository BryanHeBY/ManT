//! Literal payload, row occupancy and continuation reset as one buffer.
use super::layout;
use crate::mandoc::inline::{DraftInline as Inline, draft::has_printable_character};
use mant_ir::{Block, SourceSpan};

pub(super) struct LiteralFlow {
    nodes: Vec<Inline>,
    source: Option<SourceSpan>,
    tight_boundary: bool,
    ordinary_continuation: bool,
    row_occupied: bool,
    formatter_column: FormatterColumn,
    content: crate::mandoc::content::LegacyContent,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum FormatterColumn {
    Origin,
    Advanced,
}

impl LiteralFlow {
    pub(super) fn new(content: crate::mandoc::content::LegacyContent) -> Self {
        Self {
            nodes: Vec::new(),
            source: None,
            tight_boundary: false,
            ordinary_continuation: false,
            row_occupied: false,
            formatter_column: FormatterColumn::Origin,
            content,
        }
    }

    pub(super) const fn starts_new_row(&self, starts_line: bool) -> bool {
        starts_line && self.row_occupied && !self.tight_boundary
    }

    pub(super) fn end_line(&mut self) {
        if self.row_occupied {
            self.nodes.push(Inline::LineBreak);
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
            .is_some_and(|node| matches!(node, Inline::LineBreak));
        if let Some(last_break) = nodes
            .iter()
            .rposition(|node| matches!(node, Inline::LineBreak))
        {
            self.formatter_column =
                if has_printable_character(&nodes[last_break.saturating_add(1)..]) {
                    FormatterColumn::Advanced
                } else {
                    FormatterColumn::Origin
                };
        } else if has_printable_character(&nodes) {
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
        let committed_a_cell = has_printable_character(&nodes);
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

    pub(super) fn take(&mut self, indent: crate::mandoc::layout::SourceIndent) -> Option<Block> {
        let mut previous = std::mem::replace(self, Self::new(self.content.clone()));
        // A formatter-only word closes a row without occupying the next one.
        // Actual empty rows end with an explicit empty text sentinel.
        if !previous.row_occupied && matches!(previous.nodes.last(), Some(Inline::LineBreak)) {
            previous.nodes.pop();
        }
        (!previous.nodes.is_empty()).then(|| Block::Preformatted {
            children: previous.content.lower(
                mant_ir::ContentRootKind::FixedBody,
                previous.source,
                previous.nodes,
            ),
            language: None,
            layout: layout(indent),
            source: previous.source,
        })
    }
}
