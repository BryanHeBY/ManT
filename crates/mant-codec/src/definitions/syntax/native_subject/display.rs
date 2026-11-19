//! Use the ordinary head grammar on styled intervals between table receipts.
use crate::definitions::syntax::{
    commands,
    inference::{InferredIdentity, infer_terms},
    occurrences::HeadNames,
    scan::{DeclarationLimit, MAX_NAMES},
};
use crate::definitions::{
    NativeHeadRole, NativeOperand, NativeOperandRole, context::DefinitionContext,
    evidence::HeadOperands,
};
use mant_ir::{DefinitionTerm, EntryKind, Inline, NameCase};
use std::ops::Range;

use super::intervals::{local_operands, split_inline_intervals};

mod plans;
mod scopes;

pub(in crate::definitions::syntax) fn infer_display_head(
    terms: &[DefinitionTerm],
    context: DefinitionContext,
    hint: Option<NativeHeadRole>,
    operands: HeadOperands<'_>,
) -> Option<InferredIdentity> {
    if !matches!(operands, HeadOperands::Accepted(_)) {
        return None;
    }
    let has_display =
        |ranges: &[NativeOperand]| ranges.iter().any(|operand| operand.role.display_only());
    if !(0..terms.len()).any(|index| operands.for_term(index).is_some_and(has_display)) {
        return None;
    }
    let mut head = HeadNames {
        occurrences: Vec::with_capacity(terms.len()),
        excluded_forms: Vec::new(),
        limit: None,
        semantic: true,
    };
    let mut kind = None;
    for (index, term) in terms.iter().enumerate() {
        let mut names = Vec::new();
        if let Some(ranges) = operands.for_term(index) {
            if has_display(ranges) {
                // The complete authored term stays in the public owner.
                // Its display-only bytes cannot supply a queryable form.
                head.excluded_forms.push(index);
                let mut segments = segments(term, ranges, operands.body_ranges(index))
                    .into_iter()
                    .peekable();
                let mut first = true;
                while let Some(segment) = segments.next() {
                    let group = segment.group;
                    let mut projected = Projection::default();
                    projected.push(segment);
                    while let Some(next) = segments.next_if(|next| next.group == group) {
                        projected.push(next);
                    }
                    let original_hint = (index == 0 && first).then_some(hint).flatten();
                    if !mant_ir::has_printable_character(&projected.content) {
                        continue;
                    }
                    first = false;
                    let Some(segment) = projected.infer(context, original_hint) else {
                        continue;
                    };
                    append_names(segment, &mut names, &mut kind, &mut head.limit);
                }
            } else {
                let local = [local_operands(ranges.to_vec())];
                let inherited = (index == 0).then_some(hint).flatten();
                let inferred = infer_terms(
                    std::slice::from_ref(term),
                    context,
                    inherited,
                    HeadOperands::Accepted(&local),
                );
                if !inferred.excluded_forms.is_empty() || !inferred.semantic {
                    head.excluded_forms.push(index);
                }
                append_names(inferred, &mut names, &mut kind, &mut head.limit);
            }
        } else {
            head.excluded_forms.push(index);
        }
        head.occurrences.push(names);
    }
    let (kind, case) = kind.unwrap_or((EntryKind::Term, NameCase::Sensitive));
    Some(InferredIdentity::classify(head, kind, case, false))
}

fn append_names(
    inferred: InferredIdentity,
    names: &mut Vec<crate::definitions::RecognizedName>,
    kind: &mut Option<(EntryKind, NameCase)>,
    limit: &mut Option<DeclarationLimit>,
) {
    *limit = limit.or(inferred.limit);
    let Some(found) = inferred.occurrences.into_iter().next() else {
        return;
    };
    if !found.is_empty() {
        let role = (inferred.kind, inferred.case);
        *kind = Some(match *kind {
            Some(previous) if previous != role => (EntryKind::Term, NameCase::Sensitive),
            _ => role,
        });
        let remaining = MAX_NAMES.saturating_sub(names.len());
        if found.len() > remaining {
            *limit = Some(DeclarationLimit::Names);
        }
        names.extend(found.into_iter().take(remaining));
    }
}

struct Segment<'a> {
    group: usize,
    bytes: Range<usize>,
    operands: &'a [NativeOperand],
    content: Vec<Inline>,
}

#[derive(Default)]
struct Projection {
    content: Vec<Inline>,
    operands: Vec<NativeOperand>,
    intervals: Vec<(Range<usize>, usize)>,
    bytes: usize,
}

impl Projection {
    fn push(&mut self, segment: Segment<'_>) {
        if self.bytes > 0 {
            // A removed display span is never evidence that two native
            // words compose one name. This private gap has no public range.
            self.content.push(Inline::Text { value: " ".into() });
            self.bytes += 1;
        }
        let offset = self.bytes;
        self.bytes += segment.bytes.len();
        self.intervals
            .push((offset..self.bytes, segment.bytes.start));
        self.operands
            .extend(segment.operands.iter().map(|operand| NativeOperand {
                bytes: offset + operand.bytes.start.max(segment.bytes.start) - segment.bytes.start
                    ..offset + operand.bytes.end.min(segment.bytes.end) - segment.bytes.start,
                role: operand.role,
            }));
        self.content.extend(segment.content);
    }

    fn infer(
        mut self,
        context: DefinitionContext,
        hint: Option<NativeHeadRole>,
    ) -> Option<InferredIdentity> {
        // Preserve the ordinary styled command grammar as well as explicit
        // native heads. Arbitrary surrounding text gains no declaration
        // evidence merely by standing on one side of a display.
        let native = self.operands.iter().any(|operand| {
            matches!(
                operand.role,
                NativeOperandRole::ExplicitOption | NativeOperandRole::Environment { .. }
            ) || operand.role.subject().is_some()
                || operand.role.is_operand_label()
                || operand.role.is_search()
        });
        let text = mant_ir::inline_plain_text(&self.content);
        let styled_command = context == DefinitionContext::Commands
            && (commands::leading_styled_command_name(&self.content).is_some()
                || self.has_separated_command(&text));
        let visible = text.trim();
        let start = super::offset(&text, visible);
        if !(native
            || styled_command
            || hint == Some(NativeHeadRole::Literal)
            || super::invocation::compound_name(
                &text,
                &(start..start + visible.len()),
                &self.operands,
            )
            .is_some())
        {
            return None;
        }
        let term = DefinitionTerm::from(self.content);
        // A separator written before a table can be covered by the previous
        // receipt. Clip that edge to the span read by the head grammar.
        let end = text.trim_end().len();
        self.operands.retain_mut(|operand| {
            operand.bytes.end = operand.bytes.end.min(end);
            !operand.bytes.is_empty()
        });
        let operands = [local_operands(self.operands)];
        let mut found = infer_terms(&[term], context, hint, HeadOperands::Accepted(&operands));
        for names in &mut found.occurrences {
            names.retain_mut(|name| {
                name.parts.iter_mut().all(|part| {
                    let index = self
                        .intervals
                        .partition_point(|(span, _)| span.end <= part.start);
                    if let Some((span, original)) = self.intervals.get(index)
                        && span.start <= part.start
                        && part.end <= span.end
                    {
                        *part =
                            original + part.start - span.start..original + part.end - span.start;
                        return true;
                    }
                    false
                })
            });
        }
        Some(found)
    }

    fn has_separated_command(&self, text: &str) -> bool {
        // A confirmed Cm/Ic after the author's separate delimiter supplies
        // its own evidence even when the preceding clause has no name.
        let mut syntax = super::invocation::Syntax::new(text, &self.operands);
        let mut start = 0;
        self.operands
            .iter()
            .zip(self.operands.iter().skip(1))
            .any(|(separator, command)| {
                let delimiter = text[separator.bytes.clone()].trim();
                if !matches!(
                    separator.role.declaration_delimiter(),
                    NativeOperandRole::LiteralContinuation(None)
                        | NativeOperandRole::PipeDelimiter(None)
                ) || !matches!(delimiter, "," | "|")
                    || syntax.protects(start, super::offset(text, delimiter))
                {
                    return false;
                }
                start = separator.bytes.end;
                command.role == NativeOperandRole::Command
                    && text[separator.bytes.end..command.bytes.start]
                        .trim()
                        .is_empty()
            })
    }
}

fn segments<'a>(
    term: &[Inline],
    operands: &'a [NativeOperand],
    bodies: &[Range<usize>],
) -> Vec<Segment<'a>> {
    let text = mant_ir::inline_plain_text(term);
    let boundaries = scopes::display_boundaries(&text, operands, bodies);
    let mut first_receipt = 0usize;
    let mut segments = Vec::new();
    for (group, bytes) in plans::retained_intervals(&text, operands, boundaries) {
        first_receipt +=
            operands[first_receipt..].partition_point(|operand| operand.bytes.end <= bytes.start);
        let count =
            operands[first_receipt..].partition_point(|operand| operand.bytes.start < bytes.end);
        segments.push(Segment {
            group,
            bytes,
            operands: &operands[first_receipt..first_receipt + count],
            content: Vec::new(),
        });
    }
    let slices = split_inline_intervals(term, segments.iter().map(|segment| segment.bytes.clone()));
    for (segment, content) in segments.iter_mut().zip(slices) {
        segment.content = content;
    }
    segments
}
