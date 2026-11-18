//! Recognize typed declarations separated by display-only table content.
use super::{Recognition, environment, recognize};
use crate::definitions::syntax::{
    eligibility::{OperandUses, operand_at},
    options,
    scan::{self, DeclarationLimit, HeadView, MAX_NAMES},
};
use crate::definitions::{NativeOperand, NativeOperandRole, RecognizedName};
use mant_ir::{EntryKind, Inline, ParameterKind, inline_plain_text};

#[derive(Default)]
pub(in crate::definitions::syntax) struct DisplayNames {
    pub(in crate::definitions::syntax) kind: Option<EntryKind>,
    pub(in crate::definitions::syntax) names: Vec<RecognizedName>,
    pub(in crate::definitions::syntax) limit: Option<DeclarationLimit>,
}

pub(in crate::definitions::syntax) fn display_separated_names(
    term: &[Inline],
    operands: &[NativeOperand],
) -> DisplayNames {
    let text = inline_plain_text(term);
    let mut result = DisplayNames::default();
    let mut start = 0;
    let mut end = 0;
    for segment in operands.split(|operand| operand.role.display_only()) {
        let boundary = operands.get(end + segment.len());
        let end_at = boundary.map_or(text.len(), |operand| operand.bytes.start);
        if let Some((kind, names, limit)) = recognize_segment(&text[start..end_at], start, segment)
        {
            result.kind = Some(match result.kind {
                Some(previous) if previous != kind => EntryKind::Term,
                _ => kind,
            });
            let available = MAX_NAMES.saturating_sub(result.names.len());
            if names.len() > available {
                result.limit = Some(DeclarationLimit::Names);
            }
            result.limit = result.limit.or(limit);
            result
                .names
                .extend(names.into_iter().take(available).map(|mut name| {
                    for range in &mut name.parts {
                        range.start += start;
                        range.end += start;
                    }
                    name
                }));
        }
        end += segment.len() + 1;
        start = boundary.map_or(text.len(), |operand| operand.bytes.end);
    }
    result
}

fn recognize_segment(
    text: &str,
    base: usize,
    operands: &[NativeOperand],
) -> Option<(EntryKind, Vec<RecognizedName>, Option<DeclarationLimit>)> {
    let head = text.len() - text.trim_start().len();
    let lead = operand_at(operands, base + head)?;
    // Crossing a table is an explicit display boundary, not new evidence
    // for lexical discovery in arbitrary adjacent text.
    let operands = operands
        .iter()
        .map(|operand| NativeOperand {
            bytes: operand.bytes.start - base..operand.bytes.end - base,
            role: operand.role,
        })
        .collect::<Vec<_>>();
    if lead.role == NativeOperandRole::ExplicitOption {
        let mut found = scan::native_option_head(text, &operands);
        if found.names.is_empty()
            && found.limit.is_none()
            && let Some(name) = options::native_option_key(&text[head..lead.bytes.end - base])
        {
            found
                .names
                .push(RecognizedName::contiguous(&name.name, head));
        }
        return Some((
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            },
            found.names,
            found.limit,
        ));
    }
    if matches!(lead.role, NativeOperandRole::Environment { .. }) {
        let view = HeadView::native_word(text, &operands);
        let found = environment::recognize(&view, &operands, OperandUses::new(&operands))?;
        return Some((EntryKind::EnvironmentVariable, found.names, None));
    }
    let (
        kind,
        Recognition {
            names,
            limit,
            unproved_field,
        },
    ) = recognize(text, &operands)?;
    (!unproved_field).then_some((kind, names, limit))
}
