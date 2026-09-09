//! Project documents through shared selector policy into outline and excerpt DTOs.
pub(crate) mod excerpt;
mod outline;
pub mod references;
pub use crate::explanation::select_explanation;
#[cfg(test)]
use crate::producer_identity::outline_identity_diagnostics;
pub use crate::selectors::{ProjectionError, SelectorCandidate};
pub use excerpt::select_excerpt;
pub use mant_ir::semantics_complete;
pub use outline::{
    build_outline, build_outline_projection, build_outline_with_detail,
    build_outline_with_references,
};
pub use references::{
    ReferenceProjectionLimits, project_references, project_references_with_limits,
};
const TLDR_TITLE: &str = "TLDR QUICK REFERENCE";

#[cfg(test)]
mod tests;
