//! Checked Fixed selections and native display geometry for DTO transfer.

use std::ops::Range;

use mant_ir::{FixedBody, OwnerMark, TextSelection};
use mant_protocol::{
    EvidenceBasis, ExplanationContent, ExplanationEvidence, ExplanationFixedPart,
    ExplanationFixedSelection,
};

use super::ExplanationError;

pub(super) fn hit_selection(
    unit: &crate::search::fixed_visible::units::FixedUnitText<'_>,
    hit: &Range<usize>,
) -> Option<TextSelection> {
    let mut parts = Vec::new();
    let mut joins = Vec::new();
    for part in &unit.parts {
        let start = hit.start.max(part.text_range.start);
        let end = hit.end.min(part.text_range.end);
        if start >= end {
            continue;
        }
        if parts.len() == mant_protocol::MAX_EXPLANATION_POSITIONS {
            return None;
        }
        if !parts.is_empty() {
            joins.push(part.piece.join_before.clone()?);
        }
        let start_byte = part
            .piece
            .slice
            .start_byte
            .checked_add(u64::try_from(start - part.text_range.start).ok()?)?;
        let end_byte = part
            .piece
            .slice
            .start_byte
            .checked_add(u64::try_from(end - part.text_range.start).ok()?)?;
        parts.push(mant_ir::OutputSlice {
            run: part.piece.slice.run,
            start_byte,
            end_byte,
        });
    }
    (!parts.is_empty()).then_some(TextSelection { parts, joins })
}

fn name_span(expected: &str, name: &str) -> Result<(u64, u64), ExplanationError> {
    let start_byte = if name == expected {
        0
    } else {
        expected.len() - expected.trim_start().len()
    };
    let end_byte = start_byte
        .checked_add(name.len())
        .ok_or(ExplanationError::InvalidFixed)?;
    if expected.get(start_byte..end_byte) != Some(name) {
        return Err(ExplanationError::InvalidFixed);
    }
    let start_scalar = expected[..start_byte].chars().count() as u64;
    let end_scalar = start_scalar + name.chars().count() as u64;
    Ok((start_scalar, end_scalar))
}

type FixedNamePositions = Vec<Vec<(usize, u64, u64)>>;

pub(super) fn fixed_name_positions(
    fixed: &FixedBody,
    owner: &OwnerMark,
    entry: &mant_ir::SemanticEntry,
) -> Result<FixedNamePositions, ExplanationError> {
    if entry.kind == mant_ir::EntryKind::Term && entry.names.is_empty() {
        return Ok(Vec::new());
    }
    if entry.forms.len() == 1 {
        let form = &entry.forms[0];
        // A parser-alive first-name hint does not exclude later declarations
        // or a second occurrence of that same name. Follow the native facts'
        // lexical binding, not the number of distinct names: two occurrences
        // can still yield one `entry.names` value.
        let names = if owner.head_role == Some(mant_ir::OwnerHeadRole::Lexical)
            && (owner.head_role_prefix.is_none()
                || owner.entry.as_ref().is_some_and(|facts| {
                    facts
                        .name_bindings
                        .iter()
                        .any(|binding| binding.evidence == mant_ir::EntryNameEvidence::Lexical)
                }))
            && matches!(
                entry.kind,
                mant_ir::EntryKind::Parameter {
                    parameter_kind: mant_ir::ParameterKind::Option
                }
            ) {
            fixed.lexical_names(owner).map(|components| {
                components
                    .into_iter()
                    .map(|(name, _, range)| (name, range))
                    .collect::<Vec<_>>()
            })
        } else if owner.head_role == Some(mant_ir::OwnerHeadRole::Option)
            && owner.head_components.len() > 1
        {
            fixed.option_component_names(owner).map(|components| {
                components
                    .into_iter()
                    .map(|(name, _, range)| (name, range))
                    .collect()
            })
        } else {
            None
        };
        if let Some(names) = names {
            let mut cursor = 0usize;
            let mut scalar = 0u64;
            let mut positions = vec![Vec::new(); entry.names.len()];
            let indices = entry
                .names
                .iter()
                .enumerate()
                .map(|(index, name)| (name.as_str(), index))
                .collect::<std::collections::BTreeMap<_, _>>();
            for (name, range) in names {
                let index = *indices
                    .get(name.as_str())
                    .ok_or(ExplanationError::InvalidFixed)?;
                if range.start < cursor || form.get(range.clone()) != Some(name.as_str()) {
                    return Err(ExplanationError::InvalidFixed);
                }
                for character in form[cursor..range.start].chars() {
                    scalar += 1;
                    cursor += character.len_utf8();
                }
                let start = scalar;
                for character in form[range.clone()].chars() {
                    scalar += 1;
                    cursor += character.len_utf8();
                }
                positions[index].push((0, start, scalar));
            }
            if positions.iter().any(Vec::is_empty) {
                return Err(ExplanationError::InvalidFixed);
            }
            return Ok(positions);
        }
    }
    if entry.forms.len() != entry.names.len() {
        return Err(ExplanationError::InvalidFixed);
    }
    entry
        .forms
        .iter()
        .zip(&entry.names)
        .enumerate()
        .map(|(index, (form, name))| {
            let (start, end) = name_span(form, name)?;
            Ok(vec![(index, start, end)])
        })
        .collect()
}

pub(super) fn retained_fixed_positions(record: &ExplanationEvidence) -> usize {
    let forms = record.entry.as_ref().map_or(0, |entry| {
        let form_parts = entry.fixed_forms.iter().fold(0usize, |count, selection| {
            count.saturating_add(selection.parts.len())
        });
        entry
            .name_bindings
            .iter()
            .fold(form_parts, |count, binding| {
                binding.occurrences.iter().fold(count, |count, occurrence| {
                    count.saturating_add(occurrence.fixed_forms.len())
                })
            })
    });
    let matches = record
        .bases
        .iter()
        .fold(0usize, |count, basis| match basis {
            EvidenceBasis::Name { matches } => matches.iter().fold(count, |count, matched| {
                matched.occurrences.iter().fold(count, |count, occurrence| {
                    count.saturating_add(occurrence.fixed_forms.len())
                })
            }),
            EvidenceBasis::Form { matches } => matches.iter().fold(count, |count, matched| {
                matched.occurrences.iter().fold(count, |count, occurrence| {
                    count.saturating_add(occurrence.fixed_forms.len())
                })
            }),
            _ => count,
        });
    let body = match &record.content {
        Some(ExplanationContent::FixedOwner { reading_body, .. }) => reading_body.parts.len(),
        _ => 0,
    };
    forms.saturating_add(matches).saturating_add(body)
}

pub(super) fn selection(
    fixed: &FixedBody,
    source: &TextSelection,
    maximum_bytes: usize,
) -> Option<ExplanationFixedSelection> {
    if source.parts.len() > mant_protocol::MAX_EXPLANATION_POSITIONS
        || selection_bytes(source)? > maximum_bytes
    {
        return None;
    }
    let mut parts = Vec::with_capacity(source.parts.len());
    for slice in &source.parts {
        let run = fixed
            .surface
            .runs
            .get(usize::try_from(slice.run.get() - 1).ok()?)?;
        let text = fixed.surface.run_text(slice.run)?;
        let start = usize::try_from(slice.start_byte).ok()?;
        let end = usize::try_from(slice.end_byte).ok()?;
        let (column, width) = if start == 0 && end == text.len() {
            (run.column, run.width)
        } else {
            // A clipped run needs a checked UTF-8-scalar to terminal-cell
            // mapping. Never treat byte offsets as cell offsets or guess when
            // the native run width disagrees with this mapping.
            // Pinned term_ascii.c::utf8_getwidth calls the per-scalar
            // mant_mandoc_utf8_width hook. Grapheme-wide measurement would
            // undercount joined emoji and misplace a clipped native run.
            if native_cell_width(text)? != run.width {
                return None;
            }
            (
                run.column
                    .checked_add(native_cell_width(text.get(..start)?)?)?,
                native_cell_width(text.get(start..end)?)?,
            )
        };
        parts.push(ExplanationFixedPart {
            slice: *slice,
            row: run.row,
            run_column: run.column,
            column,
            width,
            style: run.label.style,
            text: text.get(start..end)?.to_owned(),
            source: run.label.source,
        });
    }
    let selection = ExplanationFixedSelection {
        parts,
        joins: source.joins.clone(),
    };
    selection.validate().ok()?;
    Some(selection)
}

fn native_cell_width(text: &str) -> Option<u32> {
    use unicode_width::UnicodeWidthChar;

    text.chars().try_fold(0_u32, |total, scalar| {
        total.checked_add(u32::try_from(scalar.width().unwrap_or(0)).ok()?)
    })
}

pub(super) fn selection_bytes(source: &TextSelection) -> Option<usize> {
    let text = source.parts.iter().try_fold(0usize, |total, part| {
        total.checked_add(usize::try_from(part.end_byte.checked_sub(part.start_byte)?).ok()?)
    })?;
    source.joins.iter().try_fold(text, |total, join| {
        total.checked_add(match join {
            mant_ir::TextJoin::AuthoredSeparator(text)
            | mant_ir::TextJoin::GeneratedSeparator(text) => text.len(),
            _ => 0,
        })
    })
}
