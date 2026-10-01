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

//! Native field targets and the numeric device receipt of a real flush.

use super::super::native_field::{FieldFlag, FieldFlags};
use super::super::{AuthorBreakEffect, InlineBuilder};
use super::state::NoBreakField;

/// Output of the current `term_field()` sweep, independent of inherited
/// device viscol. NBRZW, rejected and empty cells can remain unprinted.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum NativeFieldEmission {
    Unprinted,
    Printed,
}

/// Numeric state of the current print sweep. This is local to one receipt;
/// persistent formatter flags and cells remain in the execution state.
struct NativeFieldSweep {
    viscol: usize,
    printed_row: Option<usize>,
    emission: NativeFieldEmission,
    separator_retention: Option<usize>,
    row_origins: Vec<(String, usize, usize)>,
    unprojected_origin_units: usize,
}

/// Position at which one accepted interval reaches `term_field()`.
struct NativeFieldPrint {
    start: usize,
    end: usize,
    first: bool,
    padding_units: usize,
    origin_units: usize,
    tab_offset: i64,
}

/// The final field's row decision after its tail-space sweep.
struct NativeFieldTail {
    ends_row: bool,
    overruns: bool,
    final_pass_continued: bool,
    trailspace_cells: usize,
}

impl NativeFieldSweep {
    fn into_device(
        self,
        width: usize,
        tab_offset: i64,
        tail: &NativeFieldTail,
        separator_field: Option<NoBreakField>,
        output_start: usize,
    ) -> NativeFieldDevice {
        NativeFieldDevice {
            width,
            viscol: if tail.ends_row { 0 } else { self.viscol },
            ends_row: tail.ends_row,
            overruns: tail.overruns,
            final_pass_continued: tail.final_pass_continued,
            next_field_gap_cells: if tail.ends_row {
                0
            } else {
                tail.trailspace_cells
            },
            tab_offset,
            unprojected_origin_units: self.unprojected_origin_units,
            printed_row: self.printed_row,
            emission: self.emission,
            separator_retention: self.separator_retention,
            separator_field,
            row_origins: self.row_origins,
            output_start,
        }
    }
}

/// Device facts of one real `term_flushln()`, independent of its semantic
/// owner or of whether a Link/style wrapper contains the printed glyphs.
///
/// `width` is the last printed pass's fill width; `overruns` and `ends_row`
/// already carry the term.c:250-253 decision over the sweep-widened `vbr`
/// (computed inside `native_field_device_with_resume`).
pub(super) struct NativeFieldDevice {
    pub(super) width: usize,
    pub(super) viscol: usize,
    pub(super) ends_row: bool,
    pub(super) overruns: bool,
    pub(super) final_pass_continued: bool,
    /// `term_flushln()` restores minbl for any consumed buffer, including a
    /// graphless or empty field (term.c:233-253). Backend endline clears it.
    pub(super) next_field_gap_cells: usize,
    pub(super) tab_offset: i64,
    /// Origin advances on the final printed row, including the row just
    /// closed by this flush. Unlike live viscol, this receipt survives the
    /// tail decision long enough for the column owner to deliver its break.
    pub(super) unprojected_origin_units: usize,
    /// Position of an actually printed graph row before this flush's final
    /// tail. `None` means NBRZW/empty/rejection emitted no represented graph.
    pub(super) printed_row: Option<usize>,
    /// This flush printed an accepted encoded graph, rather than merely
    /// inheriting viscol from an earlier field on the same device row.
    pub(super) emission: NativeFieldEmission,
    /// Printed next-field cells left after the accepted row's public origin
    /// already represents its device padding. The automatic native word
    /// blank is separate from that positioning (term.c:113-116,389-427).
    pub(super) separator_retention: Option<usize>,
    /// The consumed field's exact generated range remains available after
    /// a control took its live `NoBreakField` out of the execution state.
    pub(super) separator_field: Option<NoBreakField>,
    /// Stable source-word positions of actually printed new rows. The
    /// projector consumes these only after native acceptance, never at a
    /// tentative word append or from its visible IR width.
    pub(super) row_origins: Vec<(String, usize, usize)>,
    pub(super) output_start: usize,
}

impl InlineBuilder {
    /// Execute the numeric pass/print/tail rules from term.c:113-253 and
    /// term_field():374-444. Semantic recovery and hidden URI projection
    /// cannot establish native width or device occupancy.
    pub(super) fn native_field_device(&self, force_no_break: bool) -> Option<NativeFieldDevice> {
        self.native_field_device_with_resume(force_no_break, None)
    }

    /// Capture the consumed owner's range before acceptance can advance its
    /// output cursor. Numeric/native cells are unchanged by that projection.
    pub(super) fn native_field_device_at(
        &self,
        force_no_break: bool,
        resumed: Option<NoBreakField>,
        output_start: usize,
    ) -> Option<NativeFieldDevice> {
        self.native_field_device_with_resume(force_no_break, resumed)
            .map(|mut device| {
                device.output_start = output_start;
                device
            })
    }

    pub(in crate::mandoc::inline::flow) fn native_field_row_ends(&self) -> bool {
        self.native_field_device(false)
            .is_some_and(|field| field.ends_row)
    }

    /// An ordinary column's NOBREAK overrun is already represented by
    /// declared-field placement. A post with NOBREAK cleared instead closes
    /// the execution row independently of column width (term.c:250-253),
    /// and its following cell needs that explicit row boundary.
    pub(in crate::mandoc::inline::flow) fn native_field_tail_is_unconditional(&self) -> bool {
        self.native_field_parameters(false, None)
            .is_some_and(|(flags, _, _)| {
                !flags.contains(FieldFlag::Hang) && !self.native_no_break(flags, false, None)
            })
    }

    /// A column BODY's unconditional empty `term_flushln()` still executes
    /// the same tail comparison as a nonempty flush (term.c:143-146,233-253).
    pub(in crate::mandoc::inline::flow) fn native_empty_field_row_ends(&self) -> bool {
        self.native_field_device(false)
            .is_none_or(|device| device.ends_row)
    }

    fn native_field_parameters(
        &self,
        force_no_break: bool,
        resumed: Option<NoBreakField>,
    ) -> Option<(FieldFlags, usize, usize)> {
        let state = self.execution.definition.as_ref()?;
        let (flags, margin, trailspace) =
            match self.execution.author_execution.as_ref()?.break_effect {
                AuthorBreakEffect::Field {
                    flags: field_flags,
                    body_width_columns,
                    gap_cells,
                    ..
                } if !state.head_flags_cleared => (
                    field_flags,
                    usize::from(body_width_columns),
                    usize::from(gap_cells),
                ),
                // The run-in HEAD post already cleared the field flags
                // (mdoc_term.c:961-962) while keeping the shared buffer
                // (939-945): a later flush over those cells decides with
                // no BRIND restart, no HANG, and no trailspace, against
                // the ambient margin the It node restored.
                AuthorBreakEffect::Field {
                    body_width_columns, ..
                } => (FieldFlags::inset(), usize::from(body_width_columns), 0),
                AuthorBreakEffect::Line => match state.no_break.or(resumed) {
                    Some(field) => (field.flags, field.body_width, field.trailspace_cells),
                    // `.mc` recomputes an ordinary row with NOBREAK held on
                    // (roff_term.c:147-150): its vtarget and vfield are the
                    // page margin (term.c:134-136), not a field width.
                    None if force_no_break => (FieldFlags::inset(), usize::MAX / 2, 0),
                    None => (
                        FieldFlags::inset(),
                        if state.cleared_field_capacity_columns > 0 {
                            usize::from(state.cleared_field_capacity_columns)
                        } else {
                            usize::MAX / 2
                        },
                        0,
                    ),
                },
            };
        let flags = if state.no_break_cleared {
            flags.without(FieldFlag::NoBreak).without(FieldFlag::Brind)
        } else {
            flags
        };
        Some((flags, state.margin_override.unwrap_or(margin), trailspace))
    }

    fn native_no_break(
        &self,
        flags: FieldFlags,
        force_no_break: bool,
        resumed: Option<NoBreakField>,
    ) -> bool {
        let state = self.execution.definition.as_ref().expect("field session");
        force_no_break
            || flags.contains(FieldFlag::NoBreak) && !state.no_break_cleared
                && state.no_break.is_none() && resumed.is_none()
                // It HEAD post clears NOBREAK before the shared BODY executes
                // (mdoc_term.c:961-963). Owner transfer preserves cells only.
                && !state.run_in_continuation
                && self.execution.author_execution.as_ref().is_some_and(|author| matches!(author.break_effect, AuthorBreakEffect::Field { .. }))
    }

    fn native_margin_units(&self, fallback_columns: usize) -> usize {
        let state = self.execution.definition.as_ref().expect("field session");
        state
            .margin_override
            .map(|columns| columns.saturating_mul(24))
            .or(state.native_margin_units)
            .unwrap_or_else(|| fallback_columns.saturating_mul(24))
    }

    /// `term_field()` writes `vbl` only when an actual graph prints
    /// (term.c:397-434). The declared origin and normal minbl already have
    /// table placement; a temporary offset's excess has no public extent.
    fn printed_origin_advance(&self, padding: usize, viscol: usize) -> usize {
        let state = self.execution.definition.as_ref().expect("field session");
        state.column_origin_units.map_or(0, |origin| {
            let declared_padding = origin
                .saturating_sub(viscol.saturating_mul(24))
                .max(state.hang_row.minbl.saturating_mul(24));
            padding.saturating_sub(declared_padding)
        })
    }

    fn printed_row_origin(
        &self,
        start: usize,
        end: usize,
        row_origin_units: usize,
    ) -> Option<(String, usize, usize)> {
        use super::super::field_buffer::FieldCell;
        let state = self.execution.definition.as_ref()?;
        let declared_origin = state.column_origin_units?;
        let graph = (start..end).find(|cell| {
            matches!(
                state.field_buffer.cells().get(*cell),
                Some(FieldCell::Graph { .. })
            )
        })?;
        let owner = state
            .field_word_anchors
            .partition_point(|(_, _, content)| *content <= graph);
        let (_, owner, content) = owner
            .checked_sub(1)
            .and_then(|index| state.field_word_anchors.get(index))?;
        Some((
            owner.clone(),
            state
                .field_buffer
                .projection_length(*content, start.max(*content)),
            // term_flushln() chooses offset (or BRIND's restart margin)
            // before applying minbl (term.c:113-116,225-228). SourceIndent
            // represents that origin; minbl is already a field separator.
            // A temporary offset can override it rather than add to it.
            row_origin_units
                .saturating_sub(declared_origin)
                .saturating_add(11)
                / 24,
        ))
    }

    fn printed_separator_retention(
        &self,
        resumed: Option<NoBreakField>,
        start: usize,
        end: usize,
        padding_units: usize,
        origin_units: usize,
    ) -> Option<usize> {
        use super::super::field_buffer::FieldCell;
        let state = self.execution.definition.as_ref()?;
        state.no_break.or(resumed)?;
        let declared_origin = state.column_origin_units?;
        if origin_units < declared_origin {
            return None;
        }
        let graph = (start..end).find(|cell| {
            matches!(
                state.field_buffer.cells().get(*cell),
                Some(FieldCell::Graph { .. })
            )
        })?;
        let owner = state
            .field_word_anchors
            .partition_point(|(_, _, content)| *content <= graph);
        let (word_start, _, content) = owner
            .checked_sub(1)
            .and_then(|i| state.field_word_anchors.get(i))?;
        // The owner starts after term_word()'s automatic separator when the
        // field projection prepaid it. Authored leading blanks start inside
        // content and cannot be charged to this generated range.
        let automatic = state.field_buffer.projection_length(*word_start, *content);
        let positioning = padding_units
            .saturating_sub(origin_units)
            .saturating_add(11)
            / 24;
        Some(positioning.saturating_add(automatic))
    }

    pub(in crate::mandoc::inline::flow) fn native_field_targets(
        &self,
        force_no_break: bool,
        resumed: Option<NoBreakField>,
    ) -> Option<super::super::field_buffer::FillTargets> {
        let state = self.execution.definition.as_ref()?;
        let (flags, rmargin, _) = self.native_field_parameters(force_no_break, resumed)?;
        let row = &state.hang_row;
        let rmargin = self.native_margin_units(rmargin);
        let viscol = row.viscol.saturating_mul(24);
        let vbl = state
            .field_offset_units
            .saturating_sub(viscol)
            .max(row.minbl.saturating_mul(24));
        let no_break = self.native_no_break(flags, force_no_break, resumed);
        // term.c:113-137,229-230 computes each pass from its current device
        // position. Zero is a real target: a leading blank can reject the
        // whole remainder. A prior flush or IR owner does not restore width.
        Some(super::super::field_buffer::FillTargets {
            first: if no_break {
                usize::MAX / 2
            } else {
                rmargin.saturating_sub(viscol.saturating_add(vbl))
            },
            rest: if no_break {
                usize::MAX / 2
            } else {
                rmargin.saturating_sub(if flags.contains(FieldFlag::Brind) {
                    rmargin
                } else {
                    state.field_offset_units
                })
            },
            // The frozen reading contract keeps source words after an
            // overrun HANG margin flush instead of deleting them solely
            // because the device has no horizontal budget left. Authored
            // break markers still use the ordered CVS rejection rule. Keep
            // the actual targets for taboff and row geometry in both cases.
            unbounded: self.execution.no_fill_word_active
                || flags.contains(FieldFlag::Hang)
                    && row.margin_flush_seen
                    && !state.field_buffer.has_break_markers(),
        })
    }

    /// Observe the accepted interval's actual print in native order.
    /// Acceptance and loop endline remain in the caller (term.c:143-230).
    fn print_native_field_pass(
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
            sweep.separator_retention = if sweep.viscol == 0 {
                self.printed_separator_retention(
                    resumed,
                    print.start,
                    print.end,
                    print.padding_units,
                    print.origin_units,
                )
            } else {
                None
            };
        }
        sweep.emission = NativeFieldEmission::Printed;
        if sweep.viscol == 0
            && let Some(origin) =
                self.printed_row_origin(print.start, print.end, print.origin_units)
        {
            sweep.row_origins.push(origin);
        }
        // NBRZW, rejected cells and an empty field print no graph,
        // so they cannot establish a device-origin advance.
        sweep.unprojected_origin_units = sweep
            .unprojected_origin_units
            .saturating_add(self.printed_origin_advance(print.padding_units, sweep.viscol));
        sweep.viscol = sweep.viscol.saturating_add(printed);
        sweep.printed_row = Some(sweep.viscol);
    }

    pub(super) fn native_field_device_with_resume(
        &self,
        force_no_break: bool,
        resumed: Option<NoBreakField>,
    ) -> Option<NativeFieldDevice> {
        use super::super::field_buffer::{FieldCell, FlushReceipt};
        let state = self.execution.definition.as_ref()?;
        let targets = self.native_field_targets(force_no_break, resumed)?;
        // term_newln() also calls term_flushln() when only sweep.viscol is live.
        // An empty tcol still reaches the same vbr=0 tail comparison
        // (term.c:143-146,233-253); it is not an absent device receipt.
        let receipt = state.field_buffer.flush_receipt(targets, false);
        let (passes, rejected) = match &receipt {
            FlushReceipt::Accepted { passes } => (passes, false),
            FlushReceipt::Rejected { passes, .. } => (passes, true),
        };
        let (flags, rmargin, trailspace) = self.native_field_parameters(force_no_break, resumed)?;
        let no_break = self.native_no_break(flags, force_no_break, resumed);
        let rmargin = self.native_margin_units(rmargin);
        let row = &state.hang_row;
        let mut sweep = NativeFieldSweep {
            viscol: row.viscol,
            printed_row: (row.viscol > 0).then_some(row.viscol),
            emission: NativeFieldEmission::Unprinted,
            separator_retention: None,
            row_origins: Vec::new(),
            unprojected_origin_units: row.unprojected_origin_units,
        };
        let mut vbl = state
            .field_offset_units
            .saturating_sub(sweep.viscol.saturating_mul(24))
            .max(row.minbl.saturating_mul(24));
        let mut row_origin_units = state.field_offset_units;
        let mut start = 0;
        let mut vfield =
            rmargin.saturating_sub(sweep.viscol.saturating_mul(24).saturating_add(vbl));
        let mut width = 0;
        let mut width_units = 0;
        let mut tab_offset = state.field_buffer.tab_offset();
        for (index, pass) in passes.iter().enumerate() {
            vfield = rmargin.saturating_sub(sweep.viscol.saturating_mul(24).saturating_add(vbl));
            let tab_target = if no_break {
                // The responsive reading projection has no screen margin.
                usize::MAX / 2
            } else {
                vfield
            };
            width = pass.width;
            width_units = pass.units;
            self.print_native_field_pass(
                &mut sweep,
                &NativeFieldPrint {
                    start,
                    end: pass.end,
                    first: index == 0,
                    padding_units: vbl,
                    origin_units: row_origin_units,
                    tab_offset,
                },
                resumed,
            );
            tab_offset = tab_offset
                // term.c:165-168 clamps to the actual vtarget, even if
                // BRNEVER allowed an unbounded term_fill() pass.
                .saturating_add(i64::try_from(width_units.min(tab_target)).unwrap_or(i64::MAX))
                .saturating_add(24);
            start = pass.end;
            while matches!(
                state.field_buffer.cells().get(start),
                Some(FieldCell::BreakableBlank)
            ) {
                start += 1;
            }
            if index + 1 < passes.len() || rejected {
                // A genuine remaining field executes loop endline(), then
                // BRIND selects its right-margin origin for the next pass.
                sweep.viscol = 0;
                sweep.printed_row = None;
                sweep.unprojected_origin_units = 0;
                row_origin_units = if flags.contains(FieldFlag::Brind) {
                    rmargin
                } else {
                    state.field_offset_units
                };
                vbl = row_origin_units;
            }
        }
        if rejected {
            width = 0;
            width_units = 0;
            vfield = rmargin.saturating_sub(sweep.viscol.saturating_mul(24).saturating_add(vbl));
        }
        // The nbr==0 pass exits before the tail sweep (term.c:143-146),
        // with its own freshly reset vbr=0. Only an accepted final pass
        // widens vbr over its ignorable tail (177-196). Keep basic units
        // through the half-EN comparison (250-253), before IR rounding.
        let final_vbr = self.native_field_final_vbr(
            rejected,
            passes.last().map_or(0, |pass| pass.end),
            width_units,
            flags,
        );
        let overruns =
            final_vbr.saturating_add(trailspace.saturating_mul(24)) > vfield.saturating_add(12);
        let ends_row = !flags.contains(FieldFlag::Hang) && (!no_break || overruns);
        Some(sweep.into_device(
            width,
            tab_offset,
            &NativeFieldTail {
                ends_row,
                overruns,
                final_pass_continued: passes.len() > 1 && !rejected,
                trailspace_cells: trailspace,
            },
            state.no_break.or(resumed),
            self.native_field_output_start(),
        ))
    }

    pub(super) fn native_field_output_start(&self) -> usize {
        self.execution
            .author_execution
            .as_ref()
            .map_or(self.execution.flush_unit_output_start, |author| {
                author.field_output_start
            })
    }

    fn native_field_final_vbr(
        &self,
        rejected: bool,
        end: usize,
        width_units: usize,
        flags: FieldFlags,
    ) -> usize {
        if rejected {
            0
        } else {
            self.execution
                .definition
                .as_ref()
                .expect("field session")
                .field_buffer
                .brtrsp_tail_sweep(end, width_units, flags.contains(FieldFlag::BrTrsp))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::InlineBuilder;
    use super::super::super::field_buffer::{FieldCell, FieldWrite};
    use super::super::DefinitionGeometryCheckpoint;

    #[test]
    fn printed_origin_advances_survive_node_geometry_until_the_real_row_end() {
        // Exact D1/Dl/Bd-offset column sources ran pristine in five profiles
        // first (lint=0). term_field writes the temporary origin advance
        // before its graph; restoring offset after post does not rewind
        // viscol (term.c:397-434; mdoc_term.c:437). X AFTER ends the first
        // width-12 row even though its public seven glyph cells fit it.
        let mut builder = InlineBuilder::new();
        builder.begin_column_body(12, 0, false);
        let checkpoint = DefinitionGeometryCheckpoint {
            indent_columns: 0,
            field_offset: 0,
            field_offset_units: 0,
            margin_override: None,
        };
        builder.execution.add_native_display_offset(6);
        builder.append_text("X");
        builder.execute_native_newline();
        let row = &builder.definition.as_ref().unwrap().hang_row;
        assert_eq!(row.viscol, 7);
        assert_eq!(row.unprojected_origin_units, 6 * 24);
        builder
            .execution
            .restore_definition_geometry(Some(checkpoint));
        assert_eq!(
            builder
                .definition
                .as_ref()
                .unwrap()
                .hang_row
                .unprojected_origin_units,
            6 * 24
        );
        builder.append_text("AFTER");
        assert!(builder.finish_nested_column_part());
        let row = &builder.definition.as_ref().unwrap().hang_row;
        assert_eq!(row.viscol, 0);
        assert_eq!(row.unprojected_origin_units, 0);
    }

    #[test]
    fn a_source_origin_overrides_existing_minimum_field_spacing() {
        // The exact width-12 D1 \zX counterpart ran pristine in five
        // profiles first. term_flushln chooses max(offset,minbl), not their
        // sum (term.c:113-116); SourceIndent keeps the actual six-cell
        // source origin even when minbl was already one cell.
        let mut builder = InlineBuilder::new();
        builder.begin_column_body(12, 0, false);
        builder.definition_state_mut().hang_row.minbl = 1;
        builder.execution.add_native_display_offset(6);
        builder.append_text("X");
        let device = builder.native_field_device(false).unwrap();
        assert_eq!(device.row_origins.len(), 1);
        assert_eq!(device.row_origins[0].2, 6);
        // This distinct excess is used only to deliver an otherwise hidden
        // actual row end; it is not the row's presentation origin.
        assert_eq!(device.unprojected_origin_units, 5 * 24);
    }

    #[test]
    fn an_empty_second_column_flush_can_close_the_previously_printed_row() {
        // The exact compact Bd INNER/FIELD source ran pristine before this
        // assertion. Bd's first flush fits six cells (five graph + trail);
        // It post's empty second flush sees vfield=0,minbl=1 and ends that
        // printed row (term.c:113-137,233-253). I/INNE retain spare room.
        for (word, closes) in [("I", false), ("INNE", false), ("INNER", true)] {
            let mut builder = InlineBuilder::new();
            builder.begin_column_body(6, 0, false);
            builder.append_text(word);
            builder.execute_native_newline();
            assert_eq!(builder.finish_nested_column_part(), closes, "{word}");
        }
    }

    #[test]
    fn unprinted_native_cells_cannot_establish_an_origin_advance() {
        // The exact \& and leading \p counterparts ran pristine first.
        // NBRZW affects term_fill's graph, but term_field skips it; nbr==0
        // rejection prints no prefix at all (term.c:143-146,397-399).
        for writes in [
            vec![FieldWrite::Cell(FieldCell::ZeroWidthGraph)],
            vec![
                FieldWrite::Cell(FieldCell::BreakMarker),
                FieldWrite::Cell(FieldCell::BreakableBlank),
                FieldWrite::Cell(FieldCell::Graph {
                    text: 'D',
                    width: 1,
                }),
            ],
        ] {
            let mut builder = InlineBuilder::new();
            builder.begin_column_body(12, 0, false);
            builder.execution.add_native_display_offset(6);
            builder
                .definition_state_mut()
                .field_buffer
                .apply_writes(&writes);
            let device = builder.native_field_device(false).unwrap();
            assert_eq!(device.unprojected_origin_units, 0);
        }
    }

    #[test]
    fn declared_column_placement_is_not_an_unprojected_origin() {
        // Exact ordinary short/CLSET_TIMEOUT column sources ran pristine
        // first. Their declared origin is represented by the table layout;
        // it cannot add an inline break to the semantic cell word.
        let mut builder = InlineBuilder::new();
        builder.begin_column_body(20, 12, false);
        builder.append_text("X");
        assert_eq!(
            builder
                .native_field_device(false)
                .unwrap()
                .unprojected_origin_units,
            0
        );
        let mut overrun = InlineBuilder::new();
        overrun.begin_column_body(12, 0, false);
        overrun.append_text("CLSET_TIMEOUT");
        assert!(!overrun.finish_nested_column_part());
    }
}
