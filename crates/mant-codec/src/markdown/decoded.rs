//! Admit decoded text using the same terminal-control policy as source bytes.

use std::ops::Range;

use mant_ir::{Diagnostic, DiagnosticImpact, DiagnosticLevel};

use super::source::MarkdownSource;

/// Reference metadata belongs to its effective definition, independent of how
/// many uses expand it. Ordinary text/labels keep their individual event spans.
pub(super) fn title(
    value: String,
    range: &Range<usize>,
    link_type: pulldown_cmark::LinkType,
    cursor: &mut super::EventCursor<'_>,
    source: &MarkdownSource<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> String {
    let definition = cursor.metadata_origins.get(&range.start).cloned();
    let origin = definition
        .as_ref()
        .or_else(|| (link_type == pulldown_cmark::LinkType::Inline).then_some(range));
    if definition
        .as_ref()
        .is_none_or(|range| cursor.reported_titles.insert((range.start, range.end)))
    {
        normalize_at(value, origin.map(|range| source.span(range)), diagnostics)
    } else {
        crate::text_safety::mask_terminal_controls(&value)
            .0
            .unwrap_or(value)
    }
}

/// Parser entities can introduce controls absent from the original bytes.
/// This span belongs to the complete original parser event, not to a decoded
/// scalar offset (an entity and its value generally have different lengths).
pub(super) fn normalize(
    value: String,
    range: &Range<usize>,
    source: &MarkdownSource<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> String {
    normalize_at(value, Some(source.span(range)), diagnostics)
}

fn normalize_at(
    value: String,
    origin: Option<mant_ir::SourceSpan>,
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
        source: origin,
    });
    masked
}
