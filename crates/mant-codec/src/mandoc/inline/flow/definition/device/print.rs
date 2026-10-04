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

//! Actual accepted field printing and stable output-origin receipts.

use super::super::super::InlineBuilder;
use super::super::state::NoBreakField;
use super::{LoopRowEnd, NativeFieldEmission, NativeFieldPrint, NativeFieldSweep};

impl InlineBuilder {
    /// `term_field()` writes `vbl` only when an actual graph prints
    /// (term.c:397-434). The declared origin and normal minbl already have
    /// table placement; a temporary offset's excess has no public extent.
    fn printed_origin_advance(&self, padding: usize, viscol: usize) -> usize {
        let state = self.execution.definition.as_ref().expect("field session");
        state.column_origin_units.map_or(0, |origin| {
            let declared_padding = if viscol == state.hang_row.viscol {
                state.hang_row.padding_units(origin)
            } else {
                origin.saturating_sub(viscol.saturating_mul(24))
            };
            padding.saturating_sub(declared_padding)
        })
    }

    fn printed_row_origin(
        &self,
        start: usize,
        end: usize,
        row_origin_units: usize,
    ) -> Option<(String, usize, i32, bool)> {
        use super::super::super::field_buffer::FieldCell;
        let state = self.execution.definition.as_ref()?;
        let declared_origin = state
            .column_origin_units
            .or(state.declared_body_origin_units)
            .unwrap_or(0);
        let graph = (start..end).find(|cell| {
            matches!(
                state.field_buffer.cells().get(*cell),
                Some(FieldCell::Graph { .. } | FieldCell::Hyphen)
            )
        })?;
        let owner = state
            .field_word_anchors
            .partition_point(|anchor| anchor.content <= graph);
        let anchor = owner
            .checked_sub(1)
            .and_then(|index| state.field_word_anchors.get(index))?;
        let scalar = state
            .field_buffer
            .projection_length(anchor.content, start.max(anchor.content));
        let owner = if anchor.projected_field_prefix && scalar == 0 && start <= anchor.content {
            // Only the word's first accepted pass owns its generated prefix.
            // A later marker pass may still have scalar zero after NBRZW;
            // its origin belongs after that real loop endline, not before
            // the word's earlier positioning (term.c:217,389-427).
            let serial = anchor
                .owner
                .strip_prefix(super::super::super::output::INTERNAL_FIELD_WORD)
                .expect("native word owner");
            format!(
                "{}{serial}",
                super::super::super::output::INTERNAL_FIELD_PREFIX
            )
        } else {
            anchor.owner.clone()
        };
        let origin = if state.column_origin_units.is_some() {
            // Bd BODY can move left of its declared column parent. The
            // accepted native positions remain nonnegative, but their
            // difference is signed (mdoc_term.c:1449-1455). Subtract in
            // basic units before rounding the relative character position;
            // Euclidean division preserves a half-cell outdent's floor.
            let units = i128::try_from(row_origin_units)
                .unwrap_or(i128::MAX)
                .saturating_sub(i128::try_from(declared_origin).unwrap_or(i128::MAX))
                .saturating_add(i128::from(
                    state
                        .column_reading_origin
                        .map_or(0, |reading| reading.correction),
                ));
            i32::try_from(units.saturating_add(11).div_euclid(24)).unwrap_or(if units < 0 {
                i32::MIN
            } else {
                i32::MAX
            })
        } else {
            // Preserve the established non-column HEAD/BODY projection.
            i32::try_from(
                row_origin_units
                    .saturating_sub(declared_origin)
                    .saturating_add(11)
                    / 24,
            )
            .unwrap_or(i32::MAX)
        };
        Some((
            owner,
            scalar,
            // term_flushln() chooses offset (or BRIND's restart margin)
            // before applying minbl (term.c:113-116,225-228). SourceIndent
            // represents that origin; minbl is already a field separator.
            // A temporary offset can override it rather than add to it.
            origin,
            state.field_buffer.projection_length(graph, graph + 1) == 0,
        ))
    }

    fn printed_field_padding(
        &self,
        start: usize,
        end: usize,
        padding_units: usize,
    ) -> Option<(String, usize, usize, bool)> {
        use super::super::super::field_buffer::FieldCell;
        let state = self.execution.definition.as_ref()?;
        // DeclaredColumns already carries table cell positioning. Its live
        // offset advances stay in the column's numeric origin receipt, not
        // as authored-looking blanks inside the following cell's text.
        if state.column_origin_units.is_some() {
            return None;
        }
        let graph = (start..end).find(|cell| {
            matches!(
                state.field_buffer.cells().get(*cell),
                Some(FieldCell::Graph { .. } | FieldCell::Hyphen)
            )
        })?;
        let owner = state
            .field_word_anchors
            .partition_point(|anchor| anchor.content <= graph);
        let anchor = owner
            .checked_sub(1)
            .and_then(|index| state.field_word_anchors.get(index))?;
        let declared_padding = state
            .declared_body_origin_units
            .map_or(0, |origin| state.hang_row.padding_units(origin));
        let cells = padding_units
            .saturating_sub(declared_padding)
            .saturating_add(11)
            / 24;
        let cells = cells.saturating_sub(anchor.projected_device_padding);
        (cells > 0).then(|| {
            (
                anchor.owner.clone(),
                state
                    .field_buffer
                    .projection_length(anchor.content, start.max(anchor.content)),
                cells,
                state.field_buffer.projection_length(graph, graph + 1) == 0,
            )
        })
    }

    fn printed_separator_retention(
        &self,
        resumed: Option<NoBreakField>,
        start: usize,
        end: usize,
        origin_units: usize,
    ) -> Option<usize> {
        use super::super::super::field_buffer::FieldCell;
        let state = self.execution.definition.as_ref()?;
        state.no_break.or(resumed)?;
        let declared_origin = state.column_origin_units?;
        if origin_units < declared_origin {
            return None;
        }
        let graph = (start..end).find(|cell| {
            matches!(
                state.field_buffer.cells().get(*cell),
                Some(FieldCell::Graph { .. } | FieldCell::Hyphen)
            )
        })?;
        let owner = state
            .field_word_anchors
            .partition_point(|anchor| anchor.content <= graph);
        let anchor = owner
            .checked_sub(1)
            .and_then(|i| state.field_word_anchors.get(i))?;
        // The owner starts after term_word()'s automatic separator when the
        // field projection prepaid it. Authored leading blanks start inside
        // content and cannot be charged to this generated range.
        let automatic = state
            .field_buffer
            .projection_length(anchor.start, anchor.content);
        // This receipt is only taken on a fresh device row. DeclaredColumns
        // and its accepted row-origin receipt already position that row;
        // minbl is not another authored prefix there. The native page's
        // common offset can cover minbl even when the responsive column
        // origin is zero (term.c:113-116,389-427). Only a blank actually
        // written by term_word() remains in this prepaid range; Ed's
        // term_newln() can select NOSPACE and suppress precisely that cell
        // (mdoc_term.c:1474-1483; term.c:475-481,573-589).
        Some(automatic)
    }

    /// Observe the accepted interval's actual print in native order.
    /// Acceptance and loop endline remain in the caller (term.c:143-230).
    pub(super) fn print_native_field_pass(
        &self,
        sweep: &mut NativeFieldSweep,
        print: &NativeFieldPrint,
        resumed: Option<NoBreakField>,
    ) {
        let state = self.execution.definition.as_ref().expect("field session");
        if print.first
            && state.column_origin_units.is_some()
            && state.no_break.or(resumed).is_some()
        {
            // Only this first field interval owns the prepaid separator.
            // An invisible accepted pass prints none of it; a graph in
            // a later row cannot revive that earlier deferred padding.
            sweep.separator_retention = Some(0);
        }
        let Some(printed) = state.field_buffer.printed_columns(
            print.start,
            print.end,
            print.tab_offset,
            print.padding_units,
        ) else {
            return;
        };
        if print.first {
            sweep.separator_retention = if sweep.printed_row.is_none() {
                self.printed_separator_retention(
                    resumed,
                    print.start,
                    print.end,
                    print.origin_units,
                )
            } else {
                None
            };
        }
        sweep.emission = NativeFieldEmission::Printed;
        if sweep.printed_row.is_none()
            && let Some(origin) =
                self.printed_row_origin(print.start, print.end, print.origin_units)
        {
            sweep.row_origins.push(origin);
        } else if sweep.printed_row.is_some()
            && let Some(padding) =
                self.printed_field_padding(print.start, print.end, print.padding_units)
        {
            sweep.field_padding.push(padding);
        }
        // NBRZW, rejected cells and an empty field print no graph,
        // so they cannot establish a device-origin advance.
        sweep.unprojected_origin_units = sweep
            .unprojected_origin_units
            .saturating_add(self.printed_origin_advance(print.padding_units, sweep.viscol));
        sweep.viscol = sweep.viscol.saturating_add(printed);
        sweep.printed_row = Some(sweep.viscol);
        // A graph printed after a loop endline occupies that endline's
        // fresh row; only an endline that stays final hands its row
        // boundary to the next field (term.c:217,233-237).
        sweep.loop_row_end = LoopRowEnd::Open;
    }
}
