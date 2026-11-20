//! Recognize Ev names from accepted operands and their own annotation.
use super::{annotations, invocation, offset, operand_at, operand_cursor};
use crate::definitions::syntax::{
    eligibility::{OperandUses, intersecting_operands},
    named,
    scan::HeadView,
};
use crate::definitions::{NativeOperand, NativeOperandRole, RecognizedName};
use mant_ir::Inline;
use std::ops::Range;

pub(in crate::definitions::syntax) fn environment_names(
    term: &[Inline],
    operands: &[NativeOperand],
) -> Option<EnvironmentNames> {
    if !operands.iter().any(|operand| is_environment(operand.role)) {
        return None;
    }
    let view = HeadView::new(term, operands);
    let uses = OperandUses::new(operands);
    Some(recognize(&view, operands, uses))
}

#[derive(Default)]
pub(in crate::definitions::syntax) struct EnvironmentNames {
    pub(in crate::definitions::syntax) names: Vec<RecognizedName>,
    pub(in crate::definitions::syntax) unproved_exact_form: bool,
}

fn recognize(
    view: &HeadView<'_>,
    operands: &[NativeOperand],
    uses: OperandUses<'_>,
) -> EnvironmentNames {
    let text = view.text.as_str();
    let (parts, assignments, complete) = split_groups(view, operands, uses);
    let unproved_exact_form =
        !complete && (!parts.is_empty() || foreign_join_at_start(text, operands));
    let mut result = EnvironmentNames {
        // A malformed label may still be the author's aliasless form.
        // A proved foreign join survives even when its group never closes.
        unproved_exact_form,
        ..EnvironmentNames::default()
    };
    let mut parts = parts.into_iter().peekable();
    let mut has_native_group = false;
    let mut has_unproved_group = false;
    while let Some(part) = parts.next() {
        let first = part.trim();
        let start = offset(text, first);
        let mut end = start + first.len();
        let leading = operand_at(operands, start).map(|operand| operand.role);
        has_native_group |= leading.is_some_and(is_environment);
        let annotation = annotation_start(text, &(start..end), operands);
        let next_assignment = assignments.partition_point(|at| *at < start);
        if assignments
            .get(next_assignment)
            .is_some_and(|at| *at < annotation.unwrap_or(end))
        {
            // A lexical separator in the RHS keeps belonging to the
            // assignment until a distinct executed Ev starts a new atom.
            // The parser's invocation ID is independent of source coords.
            while let Some(next) =
                parts.next_if(|next| !independent_environment(text, next, operands, leading, start))
            {
                let value = next.trim_end();
                end = offset(text, value) + value.len();
            }
        }
        let range = start..end;
        if let Some(name) = name_in_atom(view, &range, operands, leading, uses)
            .filter(|name| proven_spelling(uses, &(start..start + name.len()), leading))
        {
            result.names.push(RecognizedName::contiguous(&name, start));
        } else if let Some(role @ NativeOperandRole::Environment { .. }) = leading {
            result.unproved_exact_form |= joined_foreign_operand(text, &range, operands, role);
        } else if !leading.is_some_and(|role| is_environment(role) || is_description(role))
            && !named::environment_template(&text[range.clone()])
            && !uses.protects(&range)
        {
            has_unproved_group = true;
        }
    }
    // A native Ev can occur inside an ordinary label, e.g. `(PROBE_ENV)`,
    // without starting a declaration atom. Only an actual environment
    // group makes an unrelated sibling disqualify the complete form.
    result.unproved_exact_form |= has_native_group && has_unproved_group;
    result
}

fn foreign_join_at_start(text: &str, operands: &[NativeOperand]) -> bool {
    let part = text.trim();
    let start = offset(text, part);
    operand_at(operands, start).is_some_and(|lead| {
        matches!(lead.role, NativeOperandRole::Environment { .. })
            && joined_foreign_operand(text, &(start..start + part.len()), operands, lead.role)
    })
}

fn split_groups<'a>(
    view: &'a HeadView<'_>,
    operands: &[NativeOperand],
    uses: OperandUses<'_>,
) -> (Vec<&'a str>, Vec<usize>, bool) {
    let text = view.text.as_str();
    let mut assignments = Vec::new();
    let mut value_word = None;
    let enclosures = invocation::closed_child_enclosures(0..text.len(), operands, None);
    let in_enclosure = |at| {
        let index = enclosures.partition_point(|range| range.end <= at);
        enclosures
            .get(index)
            .is_some_and(|range| range.contains(&at))
    };
    let protect = |at, scope: Option<char>| {
        let role = operand_at(operands, at);
        if in_enclosure(at)
            || uses.protects_declaration_separator(at)
            || role.is_some_and(|operand| is_description(operand.role))
        {
            return true;
        }
        if value_word.is_some()
            && role.is_some_and(|operand| Some(operand.bytes.start) == value_word)
        {
            return !new_environment_after_rhs_separator(text, at, operands, role);
        }
        if scope.is_none() && text.as_bytes()[at] == b'=' {
            assignments.push(at);
            value_word = role.map(|operand| operand.bytes.start);
        }
        // A native word can contain balanced template syntax before '='.
        // Once assignment begins, the rest of that word is its literal
        // RHS, including unmatched delimiters and comma/pipe characters.
        false
    };
    let connectors = operands.iter().filter_map(|operand| {
        if !matches!(operand.role, NativeOperandRole::LiteralContinuation(_)) {
            return None;
        }
        let literal = text[operand.bytes.clone()].trim();
        let start = offset(text, literal);
        (literal == "or" && !in_enclosure(start)).then_some(start..start + literal.len())
    });
    let (groups, complete) =
        named::split_named_groups_with_connectors(text, protect, connectors, |left, end| {
            alternative_environment_connector(view, operands, uses, left, end).is_some()
        });
    (groups, assignments, complete)
}

fn alternative_environment_connector(
    view: &HeadView<'_>,
    operands: &[NativeOperand],
    uses: OperandUses<'_>,
    left: &str,
    end: usize,
) -> Option<()> {
    let text = view.text.as_str();
    let (before, after) = (
        left.ends_with(char::is_whitespace),
        text[end..].starts_with(char::is_whitespace),
    );
    if !(before && after) {
        return None;
    }
    let left = left.trim();
    let start = offset(text, left);
    let lead = operand_at(operands, start)?.role;
    let right_start = offset(text, text[end..].trim_start());
    let right = operand_at(operands, right_start)?;
    let separator = operand_at(operands, end - "or".len())?;
    // mdoc_macro.c::in_line gives each Ev its own invocation and executes
    // the intervening No independently. A literal from an Ev RHS or a
    // plain word spelling "or" supplies no such evidence.
    let NativeOperandRole::LiteralContinuation(scope) = separator.role else {
        return None;
    };
    if !matches!((lead, right.role), (
        NativeOperandRole::Environment { invocation: Some(a), enclosure: first },
        NativeOperandRole::Environment { invocation: Some(b), enclosure: last },
    ) if a != b && first == scope && last == scope)
    {
        return None;
    }
    let next = text[right_start..right.bytes.end].trim_end();
    let groups = named::split_named_groups(next, |_, _| false)?;
    named::environment_atom(groups.first()?.trim())?;
    let name = name_in_atom(
        view,
        &(start..start + left.len()),
        operands,
        Some(lead),
        uses,
    )?;
    proven_spelling(uses, &(start..start + name.len()), Some(lead)).then_some(())
}

fn proven_spelling(
    uses: OperandUses<'_>,
    bytes: &Range<usize>,
    leading: Option<NativeOperandRole>,
) -> bool {
    match leading {
        Some(role @ NativeOperandRole::Environment { .. }) => {
            uses.covers(bytes, |part| continues_environment(role, part))
        }
        Some(role @ NativeOperandRole::LiteralContinuation(_)) => {
            uses.covers(bytes, |part| part == role)
        }
        _ => false,
    }
}

fn continues_environment(lead: NativeOperandRole, part: NativeOperandRole) -> bool {
    part == lead
        || matches!(lead, NativeOperandRole::Environment { enclosure, .. }
            if part == NativeOperandRole::LiteralContinuation(enclosure))
}

fn joined_foreign_operand(
    text: &str,
    range: &Range<usize>,
    operands: &[NativeOperand],
    lead: NativeOperandRole,
) -> bool {
    let mut end = range.start;
    for operand in intersecting_operands(operands, range.clone()) {
        let start = operand.bytes.start.max(range.start);
        if start != end {
            return has_glued_suffix(text, range, end);
        }
        if !continues_environment(lead, operand.role) {
            let prefix = &text[range.start..start];
            // A typed boundary still carries authored annotation syntax,
            // even when its contents fail the complete-name grammar. A
            // literal `PREFIX_` followed by Ar is an explicit template.
            return !(is_annotation_boundary(operand.role)
                || prefix.ends_with('_') && operand.role.is_operand_label())
                && has_glued_suffix(text, range, start);
        }
        end = operand.bytes.end.min(range.end);
    }
    has_glued_suffix(text, range, end)
}

fn has_glued_suffix(text: &str, range: &Range<usize>, end: usize) -> bool {
    let prefix = &text[range.start..end];
    end < range.end
        && !text[end..range.end].starts_with(char::is_whitespace)
        && named::environment_variable_alias(prefix).is_some_and(|name| name == prefix)
}

fn new_environment_after_rhs_separator(
    text: &str,
    at: usize,
    operands: &[NativeOperand],
    current: Option<&NativeOperand>,
) -> bool {
    let Some(current) = current else { return false };
    if !matches!(text.as_bytes()[at], b',' | b'|') || current.bytes.end != at + 1 {
        return false;
    }
    let suffix = text[at + 1..].trim_start();
    let next = operand_at(operands, offset(text, suffix)).map(|operand| operand.role);
    // A final separator in the RHS word can also precede a new Ev. The
    // native invocation distinguishes it from an ordinary RHS continuation.
    matches!((current.role, next), (
        NativeOperandRole::Environment { invocation: Some(a), enclosure: left },
        Some(NativeOperandRole::Environment { invocation: Some(b), enclosure: right }),
    ) if a != b && left == right)
}

fn independent_environment(
    text: &str,
    next: &str,
    operands: &[NativeOperand],
    previous: Option<NativeOperandRole>,
    assignment_start: usize,
) -> bool {
    let Some(NativeOperandRole::Environment { enclosure, .. }) = previous else {
        return false;
    };
    let offset_of_next = offset(text, next.trim_start());
    let index = operand_cursor(operands, offset_of_next);
    let Some(operand) = operands.get(index) else {
        return false;
    };
    let NativeOperandRole::Environment {
        invocation: Some(current),
        enclosure: current_enclosure,
    } = operand.role
    else {
        return false;
    };
    if current_enclosure != enclosure
        || offset(text, text[operand.bytes.clone()].trim_start()) != offset_of_next
    {
        return false;
    }
    // If the next word is a continuation of an Ev already supplying the
    // RHS, its invocation can differ from the original assignment macro.
    // Punctuation only closes the parser's element, not that invocation.
    operands[..index]
        .iter()
        .rev()
        .take_while(|operand| operand.bytes.end > assignment_start)
        .find_map(|operand| match operand.role {
            NativeOperandRole::Environment {
                invocation,
                enclosure: owner,
            }
            | NativeOperandRole::ClippedEnvironment {
                invocation,
                enclosure: owner,
            } if owner == enclosure => Some(invocation),
            _ => None,
        })
        .is_some_and(|previous| previous.is_some_and(|previous| previous != current))
}

fn name_in_atom(
    view: &HeadView<'_>,
    range: &Range<usize>,
    operands: &[NativeOperand],
    leading: Option<NativeOperandRole>,
    uses: OperandUses<'_>,
) -> Option<String> {
    let text = view.text.as_str();
    if matches!(leading, Some(NativeOperandRole::ClippedEnvironment { .. })) {
        return None;
    }
    let part = if let Some(start) = annotation_start(text, range, operands) {
        // Start the check at the first real delimiter, not at an opening
        // glyph that belongs to an Ev spelling such as BASH_FUNC_home().
        let Some(NativeOperandRole::Environment { enclosure, .. }) = leading else {
            return None;
        };
        if !annotations::trailing_environment_annotation(
            text,
            start..range.end,
            operands,
            enclosure,
        ) {
            return None;
        }
        &text[range.start..start]
    } else {
        &text[range.clone()]
    };
    named::environment_atom(part.trim_end())
        .or_else(|| environment_with_operand_suffix(text, part, uses))
        .or_else(|| environment_with_described_suffix(text, part, operands, leading?))
        .or_else(|| {
            // A separate styled operand can supplement the Ev name, for
            // example a link label. This fallback stays inside the current
            // atom and cannot bypass a failed native delimiter proof above.
            let operand = operand_at(operands, range.start)?;
            if !is_environment(operand.role) {
                return None;
            }
            let limit = range.start + part.len();
            let end = view.emphasized_operand_start(operand.bytes.end.min(limit)..limit)?;
            let prefix = &text[range.start..end];
            if prefix.ends_with(char::is_whitespace) && !prefix.contains('=') {
                named::environment_atom(prefix.trim_end())
            } else {
                None
            }
        })
}

fn environment_with_described_suffix(
    text: &str,
    part: &str,
    operands: &[NativeOperand],
    lead: NativeOperandRole,
) -> Option<String> {
    let start = offset(text, part);
    let first_suffix = intersecting_operands(operands, start..start + part.len())
        .find(|operand| !continues_environment(lead, operand.role))?;
    let suffix =
        &text[first_suffix.bytes.start.max(start)..first_suffix.bytes.end.min(start + part.len())];
    let prefix = &text[start..offset(text, suffix.trim_start())];
    if (prefix.ends_with(char::is_whitespace) || first_suffix.role.display_only())
        && !prefix.contains('=')
        && (first_suffix.role.is_description()
            || first_suffix.role.display_only()
            || first_suffix.role.subject().is_some())
    {
        named::environment_atom(prefix.trim_end())
    } else {
        None
    }
}

fn annotation_start(text: &str, range: &Range<usize>, operands: &[NativeOperand]) -> Option<usize> {
    intersecting_operands(operands, range.clone())
        .find(|operand| is_annotation_boundary(operand.role))
        .map(|operand| offset(text, text[operand.bytes.clone()].trim_start()))
}

fn is_annotation_boundary(role: NativeOperandRole) -> bool {
    matches!(
        role,
        NativeOperandRole::EnclosureStart { .. }
            | NativeOperandRole::EnclosureEnd(_)
            | NativeOperandRole::LiteralDelimiter { .. }
            | NativeOperandRole::ClippedEnclosure
    )
}

fn environment_with_operand_suffix(
    text: &str,
    part: &str,
    uses: OperandUses<'_>,
) -> Option<String> {
    let (name, suffix) = part.trim_end().split_once(char::is_whitespace)?;
    // A separate Ar operand remains supplemental even when an earlier
    // alias uses a styled annotation. Ns would have removed this split.
    suffix
        .split_whitespace()
        .all(|word| {
            let start = offset(text, word);
            uses.covers(
                &(start..start + word.len()),
                NativeOperandRole::is_operand_label,
            )
        })
        .then(|| named::environment_variable_alias(name))
        .flatten()
}

fn is_environment(role: NativeOperandRole) -> bool {
    matches!(
        role,
        NativeOperandRole::Environment { .. } | NativeOperandRole::ClippedEnvironment { .. }
    )
}

fn is_description(role: NativeOperandRole) -> bool {
    role.is_description()
        || matches!(
            role,
            NativeOperandRole::ReferenceText(_) | NativeOperandRole::PathOperand(_)
        )
}
