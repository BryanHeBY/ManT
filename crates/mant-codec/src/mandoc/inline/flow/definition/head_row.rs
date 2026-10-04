/// Saved geometry at a native document-node entry. Row ownership and
/// device position survive output drains; only a source scope restores them.
#[derive(Clone, Copy)]
pub(in crate::mandoc) struct DefinitionGeometryCheckpoint {
    pub(in crate::mandoc::inline::flow) indent_columns: u16,
    pub(in crate::mandoc::inline::flow) field_offset: usize,
    pub(in crate::mandoc::inline::flow) field_offset_units: usize,
    pub(in crate::mandoc::inline::flow) column_reading_origin:
        Option<super::state::ColumnReadingOrigin>,
    pub(in crate::mandoc::inline::flow) margin_override: Option<usize>,
}

/// Row origins belong to the node geometry scope, independently of output
/// drains. Actual field padding is delivered by the native print receipt,
/// never predicted at word append or retracted from a later IR vector.
#[derive(Clone, Debug, Default)]
pub(in crate::mandoc::inline::flow) struct HeadRowState {
    pub(in crate::mandoc::inline::flow) indent_columns: u16,
    origin_pending: bool,
}

impl HeadRowState {
    pub(in crate::mandoc::inline::flow) fn note_row_origin(&mut self) {
        self.origin_pending = true;
    }

    pub(in crate::mandoc::inline::flow) const fn has_pending_origin(&self) -> bool {
        self.origin_pending
    }

    pub(in crate::mandoc::inline::flow) fn retire_row_origin(&mut self) {
        self.origin_pending = false;
    }
}
