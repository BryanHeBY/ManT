//! Inbound coverage summaries must not contradict retained diagnostics.

use mant_ir::{Diagnostic, content_complete, semantics_complete};

pub(crate) const fn default_content_complete() -> bool {
    true
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde predicates borrow their field"
)]
pub(crate) const fn content_is_complete(value: &bool) -> bool {
    *value
}

/// A false summary remains valid after a bounded transport drops details.
/// A true summary cannot override a retained finding or known content loss.
pub(crate) fn validate_coverage_summary(
    content_is_complete: bool,
    semantics_are_complete: Option<bool>,
    diagnostics: &[Diagnostic],
) -> Result<(), &'static str> {
    if content_is_complete && !content_complete(diagnostics) {
        return Err("contentComplete contradicts a retained content-coverage diagnostic");
    }
    if semantics_are_complete == Some(true)
        && (!content_is_complete || !semantics_complete(diagnostics))
    {
        return Err("semanticsComplete contradicts known incomplete coverage");
    }
    Ok(())
}
