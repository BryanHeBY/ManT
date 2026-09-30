//! The man(7)/mdoc(7) definition-list row machine in one place: TAG/HANG
//! field execution state (`term.c::term_flushln()` columns) and the
//! `InlineBuilder` methods that consume definition HEAD/BODY source rows.
//! Siblings only read this state through `InlineExecutionState::definition`;
//! the row invariants themselves stay private to this module.

mod controls;
mod device;
mod flush;
mod head_row;
mod list_scope;
mod state;

pub(in crate::mandoc) use state::PreservedDefinitionField;
pub(super) use state::{DefinitionFieldState, PendingFieldGapOrigin};

pub(in crate::mandoc) use head_row::DefinitionGeometryCheckpoint;
