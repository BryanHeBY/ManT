//! The man(7)/mdoc(7) definition-list row machine in one place: TAG/HANG
//! field execution state (`term.c::term_flushln()` columns) and the
//! `InlineBuilder` methods that consume definition HEAD/BODY source rows.
//! Siblings only read this state through `InlineExecutionState::definition`;
//! the row invariants themselves stay private to this module.

mod consumption;
#[cfg(test)]
mod control_trace;
mod controls;
mod device;
mod fill_mode;
mod flush;
mod function_scope;
mod geometry;
pub(in crate::mandoc::inline::flow) mod head_presentation;
mod head_row;
mod list_scope;
mod name_scope;
mod no_break;
mod run_in;
mod state;
mod vertical_space;

pub(super) use device::NativeFieldDevice;
pub(in crate::mandoc) use state::PreservedDefinitionField;
pub(super) use state::{DefinitionFieldState, PendingFieldGapOrigin};

impl super::InlineExecutionState {
    pub(in crate::mandoc) fn take_covered_head_ink(&mut self) -> Option<String> {
        let row = &mut self.definition.as_mut()?.hang_row;
        if row.open_head_ink.as_ref()?.scope_depth != self.native_field_scope_depth {
            return None;
        }
        let receipt = row.open_head_ink.take()?;
        receipt.covered.then_some(receipt.text)
    }

    pub(super) fn remember_open_head_ink(&mut self, text: String) {
        if let Some(definition) = &mut self.definition
            && definition.hang_row.open_head_ink.is_none()
        {
            definition.hang_row.open_head_ink = Some(state::OpenHeadInk {
                scope_depth: self.native_field_scope_depth,
                text,
                covered: false,
            });
        }
    }
}

pub(in crate::mandoc) use head_row::DefinitionGeometryCheckpoint;
pub(in crate::mandoc) use list_scope::NestedListScope;
pub(in crate::mandoc) use name_scope::NamePartGeometry;
pub(in crate::mandoc) use run_in::RunInBoundaryReceipt;
