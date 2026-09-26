//! Checked Fixed selections and native display geometry for DTO transfer.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::ops::Range;

use mant_ir::{FixedBody, OutputSlice, OwnerMark, TextJoin, TextSelection};
use mant_protocol::{
    EvidenceBasis, ExplanationContent, ExplanationEvidence, ExplanationFixedFormRange,
    ExplanationFixedPart, ExplanationFixedSelection,
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

/// Name index -> original occurrence ordinal -> every explicit form containing
/// that same surviving native occurrence. An occurrence can be displayed by
/// more than one explicit form; flattening it would invent source ordinals.
pub(super) type FixedNamePositions = Vec<Vec<Vec<ExplanationFixedFormRange>>>;

struct FormPart {
    slice: OutputSlice,
    logical_start: usize,
}

struct FormMap {
    text: String,
    parts: Vec<FormPart>,
    joins: Vec<TextJoin>,
}

impl FormMap {
    fn new(fixed: &FixedBody, source: &TextSelection, expected: &str) -> Option<Self> {
        let text = fixed.selection_text(source)?;
        if text != expected {
            return None;
        }
        let mut parts = Vec::with_capacity(source.parts.len());
        let mut cursor = 0usize;
        for (index, &slice) in source.parts.iter().enumerate() {
            if index > 0 {
                cursor = cursor.checked_add(match source.joins.get(index - 1)? {
                    TextJoin::DirectContact => 0,
                    TextJoin::AuthoredSeparator(text) | TextJoin::GeneratedSeparator(text) => {
                        text.len()
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                })?;
            }
            parts.push(FormPart {
                slice,
                logical_start: cursor,
            });
            cursor = cursor.checked_add(
                usize::try_from(slice.end_byte.checked_sub(slice.start_byte)?).ok()?,
            )?;
        }
        (cursor == text.len()).then_some(Self {
            text,
            parts,
            joins: source.joins.clone(),
        })
    }

    /// Map only actual run/byte slices, never a same-spelling substring or
    /// terminal-cell adjacency. The first run is indexed; subsequent slices
    /// are checked against the form's native join and physical interval.
    /// `FixedBody::validate_selection_with_prefix` requires (run, start byte)
    /// ordering without overlap, which licenses the partition-point lookup.
    fn occurrence_bytes(&self, source: &TextSelection) -> Option<Range<usize>> {
        source.parts.first()?;
        let mut previous_index: Option<usize> = None;
        let mut previous_slice: Option<OutputSlice> = None;
        let mut start: Option<usize> = None;
        let mut end = 0usize;
        for (ordinal, &slice) in source.parts.iter().enumerate() {
            let index = self
                .parts
                .partition_point(|part| {
                    part.slice.run < slice.run
                        || part.slice.run == slice.run && part.slice.start_byte <= slice.start_byte
                })
                .checked_sub(1)?;
            let part = self.parts.get(index)?;
            if part.slice.run != slice.run
                || slice.start_byte >= slice.end_byte
                || slice.start_byte < part.slice.start_byte
                || slice.end_byte > part.slice.end_byte
            {
                return None;
            }
            if let Some(previous_index) = previous_index {
                let prior = previous_slice?;
                if index == previous_index {
                    if prior.end_byte != slice.start_byte
                        || source.joins.get(ordinal - 1) != Some(&TextJoin::DirectContact)
                    {
                        return None;
                    }
                } else if index != previous_index + 1
                    || prior.end_byte != self.parts[previous_index].slice.end_byte
                    || slice.start_byte != part.slice.start_byte
                    || source.joins.get(ordinal - 1) != self.joins.get(previous_index)
                {
                    return None;
                }
            }
            let logical_start = part
                .logical_start
                .checked_add(usize::try_from(slice.start_byte - part.slice.start_byte).ok()?)?;
            start.get_or_insert(logical_start);
            end = part
                .logical_start
                .checked_add(usize::try_from(slice.end_byte - part.slice.start_byte).ok()?)?;
            previous_index = Some(index);
            previous_slice = Some(slice);
        }
        let start = start?;
        (start < end).then_some(start..end)
    }
}

struct PendingRange {
    name: usize,
    occurrence: usize,
    bytes: Range<usize>,
}

fn scalar_ranges(form: &str, pending: &[PendingRange]) -> Option<Vec<Range<u64>>> {
    let mut points = Vec::with_capacity(pending.len().checked_mul(2)?);
    for (index, range) in pending.iter().enumerate() {
        points.push((range.bytes.start, index, false));
        points.push((range.bytes.end, index, true));
    }
    points.sort_unstable_by_key(|point| point.0);
    let mut ranges = vec![0..0; pending.len()];
    let mut next_boundary = form
        .char_indices()
        .map(|(byte, _)| byte)
        .skip(1)
        .chain(std::iter::once(form.len()));
    let mut byte = 0usize;
    let mut scalar = 0u64;
    for (target, index, end) in points {
        while byte < target {
            byte = next_boundary.next()?;
            scalar = scalar.checked_add(1)?;
        }
        if byte != target {
            return None;
        }
        if end {
            ranges[index].end = scalar;
        } else {
            ranges[index].start = scalar;
        }
    }
    ranges
        .iter()
        .all(|range| range.start < range.end)
        .then_some(ranges)
}

pub(super) fn fixed_name_positions(
    fixed: &FixedBody,
    owner: &OwnerMark,
    entry: &mant_ir::SemanticEntry,
) -> Result<FixedNamePositions, ExplanationError> {
    let facts = owner.entry.as_ref().ok_or(ExplanationError::InvalidFixed)?;
    if facts.names != entry.names || facts.forms.len() != entry.forms.len() {
        return Err(ExplanationError::InvalidFixed);
    }
    let mut forms = Vec::with_capacity(facts.forms.len());
    let mut by_run: BTreeMap<NonZeroU32, Vec<usize>> = BTreeMap::new();
    for (index, (source, expected)) in facts.forms.iter().zip(&entry.forms).enumerate() {
        let form = FormMap::new(fixed, source, expected).ok_or(ExplanationError::InvalidFixed)?;
        for part in &form.parts {
            let indices = by_run.entry(part.slice.run).or_default();
            if indices.last() != Some(&index) {
                indices.push(index);
            }
        }
        forms.push(form);
    }
    let mut positions = vec![Vec::new(); entry.names.len()];
    let mut pending = (0..forms.len())
        .map(|_| Vec::new())
        .collect::<Vec<Vec<PendingRange>>>();
    let mut seen = vec![false; entry.names.len()];
    for binding in &facts.name_bindings {
        let name = entry
            .names
            .get(binding.name)
            .ok_or(ExplanationError::InvalidFixed)?;
        if std::mem::replace(&mut seen[binding.name], true) || binding.occurrences.is_empty() {
            return Err(ExplanationError::InvalidFixed);
        }
        for occurrence in &binding.occurrences {
            if fixed.selection_text(occurrence).as_deref() != Some(name.as_str()) {
                return Err(ExplanationError::InvalidFixed);
            }
            let ordinal = positions[binding.name].len();
            positions[binding.name].push(Vec::new());
            let first_run = occurrence
                .parts
                .first()
                .ok_or(ExplanationError::InvalidFixed)?
                .run;
            for &form_index in by_run
                .get(&first_run)
                .ok_or(ExplanationError::InvalidFixed)?
            {
                let form = &forms[form_index];
                if let Some(bytes) = form.occurrence_bytes(occurrence) {
                    if form.text.get(bytes.clone()) != Some(name.as_str()) {
                        return Err(ExplanationError::InvalidFixed);
                    }
                    pending[form_index].push(PendingRange {
                        name: binding.name,
                        occurrence: ordinal,
                        bytes,
                    });
                }
            }
        }
    }
    if seen.iter().any(|&present| !present) {
        return Err(ExplanationError::InvalidFixed);
    }
    for (form_index, ranges) in pending.iter().enumerate() {
        let scalars =
            scalar_ranges(&forms[form_index].text, ranges).ok_or(ExplanationError::InvalidFixed)?;
        for (range, scalar) in ranges.iter().zip(scalars) {
            positions[range.name][range.occurrence].push(ExplanationFixedFormRange {
                form_index: u32::try_from(form_index)
                    .map_err(|_| ExplanationError::InvalidFixed)?,
                start_scalar: scalar.start,
                end_scalar: scalar.end,
            });
        }
    }
    if positions.iter().flatten().any(Vec::is_empty) {
        return Err(ExplanationError::InvalidFixed);
    }
    Ok(positions)
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
