// Copyright (c) 2010-2022, 2025, 2026 Ingo Schwarze <schwarze@openbsd.org>
// Copyright (c) 2008, 2009, 2010, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
//
// Permission to use, copy, modify, and distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

//! Resolved row presentation, separate from IR owners and source word state.

mod field;
mod operand;

pub(in crate::mandoc) use field::FieldPlacement;
use operand::temporary_length_units;

/// The source offset and independent temporary register have distinct
/// lifetimes. mdoc restores offset at node return, whereas `ascii_endline()`
/// consumes `ti` only after an actual device line ends. All private
/// distances are character-device basic units (24 per cell).
#[derive(Clone, Copy, Debug)]
pub(in crate::mandoc) struct RowPresentation {
    pub(in crate::mandoc) parent: i32,
    pub(in crate::mandoc) offset_shift: i32,
    pub(in crate::mandoc) temporary: i32,
    // Difference between the source-relative column ledger and the actual
    // character-device column. A negative origin cannot be represented by
    // usize viscol, but still participates in the field's word-boundary test.
    printed_shift: i32,
    alignment_owner: Option<(u64, usize, mant_ir::RowLayoutHint)>,
    requested: bool,
}

impl Default for RowPresentation {
    fn default() -> Self {
        Self {
            parent: 5 * 24,
            offset_shift: 0,
            temporary: 0,
            printed_shift: 0,
            alignment_owner: None,
            requested: false,
        }
    }
}

impl RowPresentation {
    /// `roff_term.c::roff_term_pre_ti()`: the pre-br has already executed.
    /// Repeated requests overwrite ti but operate on the current offset.
    pub(in crate::mandoc) fn temporary_indent(
        &mut self,
        argument: Option<&str>,
        native_offset: usize,
    ) {
        let Some(argument) = argument else { return };
        let (relative, operand) = if let Some(value) = argument.strip_prefix('+') {
            (1, value)
        } else if let Some(value) = argument.strip_prefix('-') {
            (-1, value)
        } else {
            (0, argument)
        };
        let Some(length) = temporary_length_units(operand) else {
            return;
        };
        self.requested = true;
        let current = self
            .parent
            .saturating_add(i32::try_from(native_offset).unwrap_or(i32::MAX))
            .saturating_add(self.offset_shift);
        let target = match relative {
            1 => {
                // C adds the signed span to size_t offset before comparing
                // maxoff. An inner negative operand can underflow and then
                // selects the same 72-EN ceiling branch as an oversized span.
                let sum = u64::try_from(current.max(0))
                    .unwrap_or_default()
                    .wrapping_add_signed(i64::from(length));
                if sum <= 72 * 24 {
                    i32::try_from(sum).unwrap_or_default()
                } else if current < 72 * 24 {
                    72 * 24
                } else {
                    current
                }
            }
            -1 => {
                if length >= 0 && length < current {
                    current - length
                } else {
                    0
                }
            }
            _ => {
                if length < 0 {
                    72 * 24
                } else {
                    length.min(72 * 24)
                }
            }
        };
        self.temporary = target.saturating_sub(current);
        self.offset_shift = self.offset_shift.saturating_add(self.temporary);
    }

    /// `term_ascii.c::ascii_endline()/locale_endline()`, not an owner drain.
    pub(in crate::mandoc) fn endline(&mut self) {
        self.endline_at(0);
    }

    /// A definition's canonical offset is separate from its transient
    /// correction. The backend floors the actual offset before storing it;
    /// subtracting a positive ti from a restored mdoc scope can reach page 0.
    pub(in crate::mandoc) fn endline_at(&mut self, native_offset: usize) {
        let base = self
            .parent
            .saturating_add(i32::try_from(native_offset).unwrap_or(i32::MAX));
        let restored = base
            .saturating_add(self.offset_shift)
            .saturating_sub(self.temporary)
            .max(0);
        self.offset_shift = restored.saturating_sub(base);
        self.temporary = 0;
        self.printed_shift = 0;
        self.alignment_owner = None;
    }

    pub(in crate::mandoc) fn device_column(self, viscol: usize) -> i64 {
        i64::try_from(viscol.saturating_mul(24))
            .unwrap_or(i64::MAX)
            .saturating_add(i64::from(self.printed_shift))
    }

    /// term_flushln():113-137: padding and capacity use the actual signed
    /// reading-relative device position, including a ti-restored outdent.
    pub(in crate::mandoc) fn field_placement(
        self,
        offset: usize,
        viscol: usize,
        minbl: usize,
        occupied: bool,
    ) -> FieldPlacement {
        let offset = i64::try_from(offset)
            .unwrap_or(i64::MAX)
            .saturating_add(i64::from(self.offset_shift))
            .max(-i64::from(self.parent));
        let column = self.device_column(viscol);
        let minbl = i64::try_from(minbl.saturating_mul(24)).unwrap_or(i64::MAX);
        let padding = if occupied {
            offset.saturating_sub(column).max(minbl).max(0)
        } else {
            offset
                .saturating_add(i64::from(self.parent))
                .max(minbl)
                .saturating_sub(i64::from(self.parent))
        };
        let projection_padding = usize::try_from(padding.max(0)).unwrap_or(usize::MAX);
        let printed_shift = if occupied {
            self.printed_shift
        } else {
            let actual = i64::from(self.parent)
                .saturating_add(padding)
                .saturating_add(11)
                .div_euclid(24)
                .saturating_sub(i64::from(self.parent).saturating_add(11).div_euclid(24));
            let projected = i64::try_from(projection_padding)
                .unwrap_or(i64::MAX)
                .saturating_add(11)
                .div_euclid(24);
            i32::try_from(actual.saturating_sub(projected).saturating_mul(24)).unwrap_or_default()
        };
        FieldPlacement {
            projection_padding,
            padding,
            target_consumption: column.saturating_add(padding),
            printed_shift,
        }
    }

    pub(in crate::mandoc) fn record_field_placement(&mut self, placement: FieldPlacement) {
        self.printed_shift = placement.printed_shift;
    }

    pub(in crate::mandoc) fn has_pending_layout(self, alignment: mant_ir::TextAlignment) -> bool {
        self.requested
            || self.offset_shift != 0
            || self.temporary != 0
            || self.alignment_owner.is_some()
            || alignment != mant_ir::TextAlignment::Left
    }

    pub(in crate::mandoc) fn remember_alignment(
        &mut self,
        receipt: &(String, usize, mant_ir::RowLayoutHint),
    ) {
        if receipt.2.alignment != mant_ir::TextAlignment::Left {
            self.alignment_owner = receipt
                .0
                .strip_prefix(super::output::INTERNAL_FIELD_WORD)
                .and_then(|serial| serial.parse().ok())
                .map(|serial| (serial, receipt.1, receipt.2));
        }
    }

    pub(in crate::mandoc) fn retire_partial_alignment(
        &mut self,
    ) -> Option<(String, usize, mant_ir::RowLayoutHint)> {
        let (serial, scalar, mut hint) = self.alignment_owner.take()?;
        hint.alignment = mant_ir::TextAlignment::Left;
        Some((
            format!("{}{serial}", super::output::INTERNAL_FIELD_WORD),
            scalar,
            hint,
        ))
    }

    pub(in crate::mandoc) fn hint_at(
        self,
        alignment: mant_ir::TextAlignment,
        native_offset: usize,
    ) -> mant_ir::RowLayoutHint {
        let base = self
            .parent
            .saturating_add(i32::try_from(native_offset).unwrap_or(i32::MAX));
        let columns = |shift: i32| {
            base.saturating_add(shift)
                .saturating_add(11)
                .div_euclid(24)
                .saturating_sub(base.saturating_add(11).div_euclid(24))
        };
        let continuation = columns(self.offset_shift.saturating_sub(self.temporary));
        mant_ir::RowLayoutHint {
            row: 0,
            indent_columns: continuation,
            first_line_indent_columns: columns(self.offset_shift).saturating_sub(continuation),
            alignment,
        }
    }
}

impl super::InlineExecutionState {
    pub(in crate::mandoc) fn apply_temporary_indent(&mut self, argument: Option<&str>) {
        let offset = self
            .definition
            .as_ref()
            .map_or(0, |state| state.field_offset_units);
        self.row_presentation.temporary_indent(argument, offset);
    }
    pub(in crate::mandoc) fn set_presentation_parent(&mut self, units: usize) {
        if self.definition.is_none() {
            self.row_presentation.parent = i32::try_from(units).unwrap_or(i32::MAX);
        }
    }
}

/// Locate an accepted graph in the source-word coordinate space. Layout is
/// attached only after the actual native pass has accepted this interval.
pub(super) fn pass_receipt(
    buffer: &super::field_buffer::FieldBuffer,
    anchors: &[super::NativeWordAnchor],
    start: usize,
    end: usize,
    hint: mant_ir::RowLayoutHint,
) -> Option<(String, usize, mant_ir::RowLayoutHint)> {
    if hint.is_empty() {
        return None;
    }
    let graph = (start..end).find(|cell| {
        matches!(
            buffer.cells().get(*cell),
            Some(
                super::field_buffer::FieldCell::Graph { .. }
                    | super::field_buffer::FieldCell::Hyphen
            )
        )
    })?;
    let anchor = anchors.get(
        anchors
            .partition_point(|anchor| anchor.content <= graph)
            .checked_sub(1)?,
    )?;
    Some((
        anchor.owner.clone(),
        buffer.projection_length(anchor.content, graph),
        hint,
    ))
}
