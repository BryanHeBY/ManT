//! Admit decoded text using the same terminal-control policy as source bytes.

use std::ops::Range;

use mant_ir::{Diagnostic, DiagnosticImpact, DiagnosticLevel};

use super::source::MarkdownSource;

/// Parser entities can introduce controls absent from the original bytes.
/// This span belongs to the complete original parser event, not to a decoded
/// scalar offset (an entity and its value generally have different lengths).
pub(super) fn normalize(
    value: String,
    range: &Range<usize>,
    source: &MarkdownSource<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> String {
    let (masked, controls) = crate::text_safety::mask_terminal_controls(&value);
    let Some(masked) = masked else {
        return value;
    };
    diagnostics.push(Diagnostic {
        impact: DiagnosticImpact::None,
        level: DiagnosticLevel::Warning,
        code: Some("markdown.control-characters".to_owned()),
        message: format!("masked {controls} terminal-unsafe decoded control character(s)"),
        source: Some(source.span(range)),
    });
    masked
}
