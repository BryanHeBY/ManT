//! Literal rows follow executed AST events, never gaps in physical source.

#[derive(Clone)]
pub(super) struct SourceCursor {
    pending: bool,
    row: Row,
    continued: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    Vacant,
    Occupied,
}

impl SourceCursor {
    pub(super) const fn new() -> Self {
        Self {
            pending: false,
            row: Row::Vacant,
            continued: false,
        }
    }

    pub(super) fn begin(&mut self) {
        self.pending = true;
    }

    /// A wrapper can open a row before its first word arrives. State-only
    /// wrappers do not create rows; an executed empty text node does.
    pub(super) fn word(&mut self, occupies_row: bool) -> bool {
        if !self.pending {
            if occupies_row {
                self.row = Row::Occupied;
            }
            return false;
        }
        let boundary = self.row == Row::Occupied && !self.continued;
        self.pending = false;
        if boundary {
            self.row = Row::Vacant;
        }
        if occupies_row {
            self.row = Row::Occupied;
        }
        self.continued = false;
        boundary
    }

    pub(super) const fn pending(&self) -> bool {
        self.pending
    }

    pub(super) const fn row_occupied(&self) -> bool {
        matches!(self.row, Row::Occupied)
    }

    /// Whether the next formatter word will first close an occupied physical
    /// row. An unrealized `\p` marker at that boundary is consumed by the
    /// same no-fill line ending; emitting both would invent an empty row.
    pub(super) const fn has_physical_line_boundary(&self) -> bool {
        self.pending && matches!(self.row, Row::Occupied) && !self.continued
    }

    pub(super) fn continue_line(&mut self, continued: bool) {
        self.continued = continued;
    }

    /// A realized inline `\\p` or native break ends the occupied output row.
    /// `\\p` itself is held by `InlineBuilder` until a real word boundary;
    /// once realized, the next source node must not manufacture a second row.
    pub(super) fn explicit_line_break(&mut self, occupies_following_row: bool) {
        self.pending = false;
        self.row = if occupies_following_row {
            Row::Occupied
        } else {
            Row::Vacant
        };
        self.continued = false;
    }

    pub(super) fn reset(&mut self) {
        *self = Self::new();
    }
}
