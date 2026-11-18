//! Use the ordinary head grammar on styled intervals between table receipts.
use crate::definitions::syntax::{
    commands,
    inference::{InferredIdentity, infer_terms},
    occurrences::HeadNames,
    scan::{DeclarationLimit, MAX_NAMES},
};
use crate::definitions::{
    NativeHeadRole, NativeOperand, NativeOperandRole,
    context::DefinitionContext,
    evidence::{HeadOperands, TermOperands},
};
use mant_ir::{DefinitionTerm, EntryKind, Inline, NameCase};
use std::{ops::Range, rc::Rc};

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
                let segments = segments(term, ranges);
                let first = segments
                    .iter()
                    .position(|segment| mant_ir::has_printable_character(&segment.content));
                for (position, segment) in segments.into_iter().enumerate() {
                    let original_hint = (index == 0 && first == Some(position))
                        .then_some(hint)
                        .flatten();
                    let Some(segment) = segment.infer(context, original_hint) else {
                        continue;
                    };
                    append_names(segment, &mut names, &mut kind, &mut head.limit);
                }
            } else {
                let local = [TermOperands::Accepted(Rc::new(ranges.to_vec()))];
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
    bytes: Range<usize>,
    operands: &'a [NativeOperand],
    content: Vec<Inline>,
}

impl Segment<'_> {
    fn infer(
        self,
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
        let styled_command = context == DefinitionContext::Commands
            && commands::leading_styled_command_name(&self.content).is_some();
        if !(native || styled_command || hint == Some(NativeHeadRole::Literal)) {
            return None;
        }
        let term = DefinitionTerm::from(self.content);
        // A separator written before a table can be covered by the previous
        // receipt. Clip that edge to the span read by the head grammar.
        let end = self.bytes.start + mant_ir::inline_plain_text(&term).trim_end().len();
        let local = self
            .operands
            .iter()
            .take_while(|operand| operand.bytes.start < end)
            .map(|operand| NativeOperand {
                bytes: operand.bytes.start.max(self.bytes.start) - self.bytes.start
                    ..operand.bytes.end.min(end) - self.bytes.start,
                role: operand.role,
            })
            .collect();
        let operands = [TermOperands::Accepted(Rc::new(local))];
        let mut found = infer_terms(&[term], context, hint, HeadOperands::Accepted(&operands));
        for name in found.occurrences.iter_mut().flatten() {
            for part in &mut name.parts {
                part.start += self.bytes.start;
                part.end += self.bytes.start;
            }
        }
        Some(found)
    }
}

fn segments<'a>(term: &[Inline], operands: &'a [NativeOperand]) -> Vec<Segment<'a>> {
    let text = mant_ir::inline_plain_text(term);
    let boundaries = scopes::display_boundaries(&text, operands);
    let mut start = 0usize;
    let mut needs_separator = false;
    let mut first_receipt = 0usize;
    let mut segments = Vec::new();
    for boundary in boundaries
        .into_iter()
        .chain(std::iter::once(scopes::Boundary {
            bytes: text.len()..text.len(),
            annotation: false,
        }))
    {
        let end = boundary.bytes.start;
        let form_start = if needs_separator {
            let suffix = text[start..end].trim_start();
            suffix
                .strip_prefix([',', '|'])
                .map(|after| end - after.len())
        } else {
            Some(start)
        };
        if let Some(start) = form_start.filter(|start| *start < end) {
            first_receipt +=
                operands[first_receipt..].partition_point(|operand| operand.bytes.end <= start);
            let count =
                operands[first_receipt..].partition_point(|operand| operand.bytes.start < end);
            segments.push(Segment {
                bytes: start..end,
                operands: &operands[first_receipt..first_receipt + count],
                content: Vec::new(),
            });
        }
        start = boundary.bytes.end;
        needs_separator = boundary.annotation;
    }
    if !segments.is_empty() {
        collect_inline_intervals(term, &mut segments, &mut 0, &mut 0);
    }
    segments
}

fn collect_inline_intervals(
    nodes: &[Inline],
    segments: &mut [Segment<'_>],
    segment_index: &mut usize,
    offset: &mut usize,
) {
    for node in nodes {
        if let Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } = node
        {
            let first = *segment_index;
            let Some(initial) = segments.get(first).map(|segment| segment.content.len()) else {
                return;
            };
            collect_inline_intervals(children, segments, segment_index, offset);
            for (index, segment) in segments
                .iter_mut()
                .enumerate()
                .take(*segment_index + 1)
                .skip(first)
            {
                let start = if index == first { initial } else { 0 };
                if start < segment.content.len() {
                    let children = segment.content.split_off(start);
                    segment.content.push(wrap_interval(node, children));
                }
            }
            continue;
        }
        let text = match node {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                value
            }
            Inline::LineBreak { .. } => "\n",
            Inline::Anchor { .. } => continue,
            _ => unreachable!("the wrappers have already been traversed"),
        };
        let start = *offset;
        let end = start + text.len();
        while let Some(segment) = segments.get_mut(*segment_index) {
            if segment.bytes.start >= end {
                break;
            }
            let clip_start = segment.bytes.start.max(start);
            let clip_end = segment.bytes.end.min(end);
            if clip_start < clip_end
                && let Some(slice) = text.get(clip_start - start..clip_end - start)
            {
                segment.content.push(if slice.len() == text.len() {
                    node.clone()
                } else if matches!(node, Inline::Code { .. }) {
                    Inline::Code {
                        value: slice.into(),
                    }
                } else {
                    Inline::Text {
                        value: slice.into(),
                    }
                });
            }
            if segment.bytes.end > end {
                break;
            }
            *segment_index += 1;
        }
        *offset = end;
    }
}

fn wrap_interval(wrapper: &Inline, children: Vec<Inline>) -> Inline {
    match wrapper {
        Inline::Strong { .. } => Inline::Strong { children },
        Inline::Emphasis { .. } => Inline::Emphasis { children },
        Inline::Link { target, title, .. } => Inline::Link {
            target: target.clone(),
            title: title.clone(),
            children,
        },
        _ => unreachable!("only wrappers own child intervals"),
    }
}
