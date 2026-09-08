//! Explicit persistent state, separate from source services and local joins.
use super::inline::FontState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FormatterState {
    pub(super) font: FontState,
    pub(super) spacing: bool,
}

impl Default for FormatterState {
    fn default() -> Self {
        Self {
            font: FontState::new(),
            spacing: true,
        }
    }
}
