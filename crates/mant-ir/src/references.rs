//! Bounded, borrowed occurrences from authoritative inline content.
//!
//! This scan does not build a semantic index, project forms, infer entries or
//! resolve destinations. A target appears once per real link node, regardless
//! of duplicates, display filters or later navigation grouping.
//!
//! One source-order walker owns traversal and ephemeral paths. Scope entry
//! points share a single work account with optional association and labels;
//! no semantic index, reconstructed forms or destination lookup is required.

mod association;
mod budget;
mod events;
mod label;
mod scope;
mod types;
mod walker;

pub use association::*;
pub use budget::ReferenceWorkBudget;
pub use events::*;
pub use label::*;
pub use scope::*;
pub use types::*;

#[cfg(test)]
mod tests;
