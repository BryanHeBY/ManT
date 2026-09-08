//! Physical lines follow executed leaf events, not the outer macro's span.
use std::sync::Arc;

pub(super) struct SourceCursor {
    rows: Arc<[(u32, u16)]>,
    previous: Option<u32>,
    continued: bool,
}

impl SourceCursor {
    pub(super) fn new(rows: Arc<[(u32, u16)]>) -> Self {
        Self {
            rows,
            previous: None,
            continued: false,
        }
    }

    pub(super) fn advance(&mut self, line: u32) -> Option<u16> {
        let rows = self
            .previous
            .filter(|previous| line > *previous && !self.continued)
            .map(|previous| {
                let start = self.rows.partition_point(|(number, _)| *number <= previous);
                self.rows[start..]
                    .iter()
                    .take_while(|(number, _)| *number < line)
                    .map(|(_, rows)| *rows)
                    .max()
                    .unwrap_or(0)
            });
        self.previous = Some(self.previous.map_or(line, |previous| previous.max(line)));
        self.continued = false;
        rows
    }

    pub(super) fn continue_line(&mut self, continued: bool) {
        self.continued = continued;
    }

    pub(super) fn reset(&mut self) {
        self.previous = None;
        self.continued = false;
    }
}
