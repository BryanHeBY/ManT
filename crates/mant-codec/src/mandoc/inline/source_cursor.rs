//! Literal rows follow executed AST events, never gaps in physical source.

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

    pub(super) fn continue_line(&mut self, continued: bool) {
        self.continued = continued;
    }

    pub(super) fn reset(&mut self) {
        *self = Self::new();
    }
}
