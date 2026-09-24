//! Monotone, checked UTF-8 byte-to-scalar mapping for ordered artifact hits.

use super::SearchError;

#[derive(Default)]
pub(super) struct ScalarCursor {
    byte: usize,
    scalar: u64,
}

impl ScalarCursor {
    pub(super) fn at(&mut self, text: &str, byte: usize) -> Result<u64, SearchError> {
        let tail = text
            .get(self.byte..byte)
            .ok_or(SearchError::ContentProjection)?;
        self.scalar = self
            .scalar
            .checked_add(tail.chars().count() as u64)
            .ok_or(SearchError::ResourceLimit)?;
        self.byte = byte;
        Ok(self.scalar)
    }
}
