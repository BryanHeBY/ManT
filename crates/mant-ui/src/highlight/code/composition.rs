//! Compose usage evidence over lexical accents without revisiting source ranges.

use super::{Accent, HighlightContext, overlay, protected_role, signature_role, source};
use crate::highlight::usage;
use std::ops::Range;

pub(super) fn usage_accents(
    source: &source::Source,
    context: HighlightContext,
    protected_ranges: &[Range<usize>],
) -> Option<Vec<usage::Accent>> {
    let value = source.text.as_str();
    let signature_text = source::syntax_text(value, protected_ranges);
    let signature_context = if context.synopsis {
        usage::Context::Synopsis
    } else {
        usage::Context::Inferred
    };
    let mut syntax = if context.literal || context.synopsis {
        usage::accents(&signature_text, signature_context, &source.emphasis, &[])
    } else {
        Vec::new()
    };
    let prose = if context.prose {
        usage::checked_prose_accents(value, &source.marked, &source.emphasis, protected_ranges)?
    } else {
        Vec::new()
    };
    usage::overlay(&mut syntax, prose);
    Some(syntax)
}

pub(super) fn merge(
    accents: &mut Vec<Accent>,
    syntax: Vec<usage::Accent>,
    opaque: &[Range<usize>],
) {
    let mut protected = 0;
    let mut opaque_index = 0;
    let signature = syntax
        .into_iter()
        .filter_map(|accent| {
            while accents
                .get(protected)
                .is_some_and(|base| base.bytes.end <= accent.bytes.start)
            {
                protected += 1;
            }
            let masked = accents[protected..]
                .iter()
                .take_while(|base| base.bytes.start < accent.bytes.end)
                .any(|base| protected_role(base.role));
            while opaque
                .get(opaque_index)
                .is_some_and(|range: &Range<usize>| range.end <= accent.bytes.start)
            {
                opaque_index += 1;
            }
            let masked = masked
                || opaque
                    .get(opaque_index)
                    .is_some_and(|range| range.start < accent.bytes.end);
            (!masked).then_some(Accent {
                bytes: accent.bytes,
                role: signature_role(accent.role),
            })
        })
        .collect();
    overlay(accents, signature);
}
