//! Validate original-source spans without mixing rendered coordinates.
use super::document::invariant_at;
use crate::{Diagnostic, SourceSpan};

pub(super) fn validate_source_span(diagnostics: &mut Vec<Diagnostic>, source: SourceSpan) {
    if source.line == 0 || source.column == 0 {
        diagnostics.push(invariant_at(
            "ir.invalid-source-position",
            "source lines and columns must be one-based".to_owned(),
            source,
        ));
    }
    if source.end_line == Some(0) || source.end_column == Some(0) {
        diagnostics.push(invariant_at(
            "ir.invalid-source-position",
            "source end lines and columns must be one-based".to_owned(),
            source,
        ));
    }
    if source.end_line.is_none() != source.end_column.is_none() {
        diagnostics.push(invariant_at(
            "ir.incomplete-source-end",
            "source end line and column must be supplied together".to_owned(),
            source,
        ));
    }
    if let (Some(end_line), Some(end_column)) = (source.end_line, source.end_column)
        && (end_line < source.line || (end_line == source.line && end_column < source.column))
    {
        diagnostics.push(invariant_at(
            "ir.reverse-source-position",
            "source end position precedes its start".to_owned(),
            source,
        ));
    }
    if source
        .byte_range
        .is_some_and(|range| range.end < range.start)
    {
        diagnostics.push(invariant_at(
            "ir.reverse-source-range",
            "source byte range ends before it starts".to_owned(),
            source,
        ));
    }
}
