// Copyright (c) 2010, 2012-2020, 2022, 2025, 2026
//               Ingo Schwarze <schwarze@openbsd.org>
// Copyright (c) 2008, 2009, 2010, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
// Copyright (c) 2013 Franco Fichtner <franco@lastsummer.de>
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

//! List phases executed inside an existing inline output owner.
//!
//! These phases borrow the same native field buffer as the surrounding HEAD.
//! Only node geometry is saved: It post clears the native list flags instead
//! of restoring their former values (mdoc_term.c:936-963).

use libmandoc_rs::{DefinitionListStyle, NormalizedListKind};

use super::super::native_field::{FieldFlag, FieldFlags};
use super::super::{AuthorBreakEffect, InlineBuilder};
use super::state::DefinitionOutcome;

#[derive(Clone, Copy)]
pub(in crate::mandoc) struct NestedListScope {
    author_effect: Option<AuthorBreakEffect>,
    margin: Option<usize>,
    margin_override: Option<usize>,
    offset_units: usize,
    offset_columns: usize,
    column_origin_units: Option<usize>,
    declared_body_origin_units: Option<usize>,
    indent_columns: u16,
    outcome: DefinitionOutcome,
    head_flags_cleared: bool,
}

impl InlineBuilder {
    pub(in crate::mandoc) fn has_definition_list_execution(&self) -> bool {
        self.execution.definition.is_some() && self.execution.author_execution.is_some()
    }

    /// Bl pre already executed `term_newln()`. Keep its committed output and
    /// device row, and save only the geometry restored at Bl return.
    pub(in crate::mandoc) fn enter_nested_list_scope(&mut self, column: bool) -> NestedListScope {
        let author_effect = self
            .execution
            .author_execution
            .map(|author| author.break_effect);
        let definition = self.definition_state_mut();
        let saved = NestedListScope {
            author_effect,
            margin: definition.native_margin_units,
            margin_override: definition.margin_override,
            offset_units: definition.field_offset_units,
            offset_columns: definition.hang_row.field_offset,
            column_origin_units: definition.column_origin_units,
            declared_body_origin_units: definition.declared_body_origin_units,
            indent_columns: definition.row.indent_columns,
            outcome: definition.outcome,
            head_flags_cleared: definition.head_flags_cleared,
        };
        if column {
            definition.native_margin_units = None;
            definition.margin_override = Some(usize::MAX / 2);
            definition.field_offset_units = 0;
            definition.hang_row.field_offset = 0;
        }
        saved
    }

    /// Execute the ordinary It HEAD pre on the live field. The source
    /// node's line event has already run: flags and geometry are applied
    /// afterwards, before any authored or generated HEAD word.
    pub(in crate::mandoc) fn enter_nested_list_head(
        &mut self,
        kind: NormalizedListKind,
        style: Option<DefinitionListStyle>,
        width_units: usize,
        offset_units: i32,
        body_empty: bool,
    ) {
        let (mut flags, mut gap) =
            self.execution
                .author_execution
                .map_or((FieldFlags::inset(), 0), |author| {
                    match author.break_effect {
                        AuthorBreakEffect::Field {
                            flags, gap_cells, ..
                        } => (flags, gap_cells),
                        AuthorBreakEffect::Line => (FieldFlags::inset(), 0),
                    }
                });
        if let Some(state) = self.execution.definition.as_ref() {
            if state.head_flags_cleared {
                flags = FieldFlags::inset();
                gap = 0;
            } else if state.no_break_cleared {
                flags = flags.without(FieldFlag::NoBreak).without(FieldFlag::Brind);
            }
        }
        let added = match (kind, style) {
            (
                NormalizedListKind::Bullet | NormalizedListKind::Dash | NormalizedListKind::Ordered,
                _,
            ) => {
                gap = 1;
                FieldFlags::hang().without(FieldFlag::Brind)
            }
            (_, Some(DefinitionListStyle::Hang)) => {
                gap = 1;
                FieldFlags::hang()
            }
            (_, Some(DefinitionListStyle::Tag)) => {
                gap = 2;
                FieldFlags::tag(body_empty)
            }
            (_, Some(DefinitionListStyle::Diagnostic)) => {
                gap = 1;
                FieldFlags::diag()
            }
            _ => FieldFlags::inset(),
        };
        flags = flags.combine(added, gap);
        let sized_head = matches!(
            kind,
            NormalizedListKind::Bullet | NormalizedListKind::Dash | NormalizedListKind::Ordered
        ) || matches!(
            style,
            Some(DefinitionListStyle::Tag | DefinitionListStyle::Hang)
        );
        let field_width = if sized_head {
            u16::try_from(width_units.saturating_add(11) / 24).unwrap_or(u16::MAX)
        } else {
            u16::MAX
        };
        let start = self.nodes.len();
        let definition = self.definition_state_mut();
        definition.no_break = None;
        definition.no_break_cleared = false;
        definition.head_flags_cleared = false;
        definition.field_offset_units = definition
            .field_offset_units
            .saturating_add_signed(isize::try_from(offset_units).unwrap_or_default());
        definition.hang_row.field_offset = definition.field_offset_units.saturating_add(11) / 24;
        if sized_head {
            definition.native_margin_units =
                Some(definition.field_offset_units.saturating_add(width_units));
            definition.margin_override = None;
        }
        if let Some(author) = &mut self.execution.author_execution {
            author.break_effect = AuthorBreakEffect::Field {
                flags,
                gap_cells: gap,
                body_width_columns: field_width,
                field_width_columns: field_width,
            };
            author.field_output_start = start;
        }
        self.tighten_next_boundary();
    }

    /// It HEAD post first performs its list-specific `term_newln()`, then
    /// clears all pad/break flags. Inset/diag/item do not flush their HEAD:
    /// preserve both its input field and output interval for the BODY.
    pub(in crate::mandoc) fn finish_nested_list_head(&mut self, flush: bool) {
        if flush {
            self.no_fill_source_line();
        }
        self.execution.clear_native_list_part_flags();
    }

    /// Restore the HEAD node's geometry, then apply the BODY offset. The
    /// HEAD's device position and unflushed native cells stay live. Generated
    /// inset/diag separators execute `term_word()`, including zero-advance.
    pub(in crate::mandoc) fn enter_nested_list_body(
        &mut self,
        saved: NestedListScope,
        width_units: usize,
        offset_units: i32,
        fixed_cells: usize,
    ) {
        self.restore_nested_list_geometry(saved);
        let definition = self.definition_state_mut();
        definition.field_offset_units = definition
            .field_offset_units
            .saturating_add_signed(isize::try_from(offset_units).unwrap_or_default())
            .saturating_add(width_units);
        definition.hang_row.field_offset = definition.field_offset_units.saturating_add(11) / 24;
        definition.declared_body_origin_units = Some(definition.field_offset_units);
        // HEAD post cleared trailspace, not the minbl from its actual flush.
        // Responsive BODY text does not acquire another field interpreter.
        if let Some(author) = &mut self.execution.author_execution {
            author.break_effect = AuthorBreakEffect::Line;
        }
        // The BODY's fixed cells below already represent the actual
        // offset/minbl positioning (termp_it_pre(), term.c:113-116). Keep
        // native minbl, but do not also materialize the HEAD's prepaid pad.
        if fixed_cells > 0 {
            self.execution.pending_field_spaces = 0;
        }
        self.tighten_next_boundary();
        self.append_run_in_cells(fixed_cells, fixed_cells, fixed_cells > 0);
    }

    /// `termp_it_pre()` skips a column HEAD. Its post clears all list flags;
    /// each BODY then chooses NOBREAK/trailspace from whether another BODY
    /// follows (mdoc_term.c:817-824,924-928,953-963). Column widths remain
    /// responsive, while those real pre/post effects still execute.
    pub(in crate::mandoc) fn enter_nested_column_part(&mut self, last: bool) {
        let flags = if last {
            FieldFlags::inset()
        } else {
            FieldFlags::diag().without(FieldFlag::Brind)
        };
        let gap = u8::try_from(flags.trailspace()).unwrap_or_default();
        let definition = self.definition_state_mut();
        definition.no_break = None;
        definition.no_break_cleared = false;
        definition.head_flags_cleared = false;
        definition.hang_row.minbl = 0;
        if let Some(author) = &mut self.execution.author_execution {
            author.break_effect = AuthorBreakEffect::Field {
                flags,
                gap_cells: gap,
                body_width_columns: u16::MAX,
                field_width_columns: u16::MAX,
            };
            author.field_output_start = self.nodes.len();
        }
        self.tighten_next_boundary();
    }

    /// Configure a top-level column BODY on the existing execution state;
    /// child blocks still use the ordinary driver and preserve IR structure.
    pub(in crate::mandoc) fn begin_column_body(&mut self, width: u16, origin: usize, last: bool) {
        let flow = self.author_flow().unwrap_or_default();
        self.inherit_author_execution_with_effect(
            flow,
            false,
            AuthorBreakEffect::Field {
                flags: FieldFlags::column(last),
                gap_cells: u8::from(!last),
                body_width_columns: width,
                field_width_columns: width,
            },
        );
        self.begin_definition_head_consumption();
        self.observe_no_fill_source_lines(true);
        let state = self.definition_state_mut();
        state.field_offset_units = origin.saturating_mul(24);
        state.column_origin_units = Some(state.field_offset_units);
        state.hang_row.field_offset = origin;
        state.native_margin_units =
            Some(origin.saturating_add(usize::from(width)).saturating_mul(24));
        state.hang_row.minbl = 0;
        // Column BODY pre clears minbl, independently of the incoming
        // term_word NOSPACE register (mdoc_term.c:916-921).
        self.execution.pending_field_spaces = 0;
    }

    /// A column BODY post calls `term_flushln()`, rather than `term_newln()`:
    /// pending text and \p run through the ordinary field consumer, and the
    /// Tab reference is not reset between columns (`mdoc_term.c:953`).
    pub(in crate::mandoc) fn finish_nested_column_part(&mut self) -> bool {
        // A nested BODY post can clear the field flags and retire its
        // input buffer. Column It post still calls term_flushln(), using
        // those current cleared flags rather than skipping the call.
        let (start, flags, gap) = self
            .execution
            .author_execution
            .and_then(|author| match author.break_effect {
                AuthorBreakEffect::Field {
                    flags, gap_cells, ..
                } => Some((author.field_output_start, flags, gap_cells)),
                AuthorBreakEffect::Line => None,
            })
            .unwrap_or((0, FieldFlags::inset(), 0));
        // A nonempty post can end a previously open column row too:
        // nested It posts clear NOBREAK (mdoc_term.c:961-963), so this
        // term_flushln() executes endline even when its field fits.
        // Hand that native row close to the block owner before its
        // paragraph drain trims the output terminator.
        let native = self.native_field_device(false);
        let explicit_row_end = self.native_field_tail_is_unconditional()
            || native.as_ref().is_some_and(|device| {
                // A temporary native origin may close a row whose public
                // cell glyphs still fit the declared field. Preserve the
                // real tail in that case; ordinary column width overruns
                // remain responsive placement, without inline pollution.
                device.unprojected_origin_units > 0
            });
        let mut closed_represented_row = self.has_formatter_cell()
            && native.as_ref().is_some_and(|device| {
                // The final tail decision (term.c:250-253) ended this
                // field's own last row, or an authored `\p` pass boundary
                // already executed the loop endline (term.c:217 with
                // 294-305) and the rejected remainder never printed on
                // the fresh row that the tail then left open. The next
                // cell starts a new row in both cases; a device-width
                // guess alone never carries a row boundary into the IR.
                (explicit_row_end && device.ends_row) || device.row_closed_by_author()
            });
        {
            let occupied = self.has_formatter_cell();
            let empty_ends_row = self.native_empty_field_row_ends();
            let device_row_occupied = self
                .execution
                .definition
                .as_ref()
                .is_some_and(|state| state.hang_row.native_row_occupied());
            self.flush_definition_field_at(
                start,
                gap,
                u16::MAX,
                u16::MAX,
                flags,
                super::flush::FieldFlushBoundary::ColumnPost,
            );
            if !occupied {
                // Unlike term_newln(), the column BODY post calls
                // term_flushln() even with no buffered byte. Its tail still
                // executes endline for the last column (term.c:233-253).
                self.execution.zero_advance.discard_at_row_end();
                let device = self.native_field_device(false);
                self.retire_native_field_with_device_at(
                    device.as_ref(),
                    super::flush::FieldFlushBoundary::ColumnPost,
                );
                self.definition_state_mut().hang_row.minbl = usize::from(gap);
                if empty_ends_row {
                    // This is a second real flush of an already printed row:
                    // minbl/trailspace can end it even when the earlier pass
                    // fit. No nonempty field placement remains to represent
                    // that end (term.c:113-137,233-253).
                    closed_represented_row = device_row_occupied;
                    if !device_row_occupied {
                        self.retain_line_breaks(1);
                        // term_flushln()'s unconditional endline emitted a
                        // new empty row, rather than closing prior graph.
                        // The block drain owns this completed-row receipt;
                        // it must not trim it as an IR paragraph terminator.
                        self.record_completed_vertical_rows(1);
                    }
                    self.definition_state_mut().hang_row.endline();
                }
            }
        }
        self.enter_nested_column_part(true);
        closed_represented_row
    }

    pub(in crate::mandoc) fn restore_nested_list_geometry(&mut self, saved: NestedListScope) {
        let definition = self.definition_state_mut();
        definition.native_margin_units = saved.margin;
        definition.margin_override = saved.margin_override;
        definition.field_offset_units = saved.offset_units;
        definition.hang_row.field_offset = saved.offset_columns;
        definition.column_origin_units = saved.column_origin_units;
        definition.declared_body_origin_units = saved.declared_body_origin_units;
        definition.row.indent_columns = saved.indent_columns;
    }

    pub(in crate::mandoc) fn exit_nested_list_scope(
        &mut self,
        saved: NestedListScope,
        flags_cleared_by_item: bool,
    ) {
        self.restore_nested_list_geometry(saved);
        let definition = self.definition_state_mut();
        definition.outcome = saved.outcome;
        // It post cleared these global flags. Geometry restore must not
        // resurrect the enclosing HEAD's former NOBREAK/HANG settings.
        definition.head_flags_cleared = flags_cleared_by_item || saved.head_flags_cleared;
        if let Some(saved_effect) = saved.author_effect
            && let Some(author) = &mut self.execution.author_execution
        {
            // An controls change persistent execution state. Restore
            // this scope's geometry/effect, never its old author mode.
            author.break_effect = saved_effect;
            author.field_output_start = self.nodes.len();
        }
    }
}

impl super::super::InlineExecutionState {
    /// The list wrapper carries normal BODY positioning. Keep the actual
    /// native columns, but do not also materialize that declared placement
    /// into its text (`termp_it_pre()` BODY offset, term.c:113-116).
    pub(in crate::mandoc) fn declare_list_body_origin(&mut self) {
        if let Some(state) = &mut self.definition {
            state.declared_body_origin_units = Some(state.field_offset_units);
        }
    }

    /// Every non-BLOCK `It` post clears the list pad/break flags and trailspace,
    /// including a plain HEAD that emits nothing and calls no `term_newln`.
    /// This is shared with structural list output; an enclosing column's
    /// former `NOBREAK` is not restored on a nested list owner return.
    /// See `mdoc_term.c::termp_it_post()`, 936–963.
    pub(in crate::mandoc) fn clear_native_list_part_flags(&mut self) {
        if let Some(definition) = &mut self.definition {
            definition.no_break = None;
            definition.no_break_cleared = false;
            definition.head_flags_cleared = true;
        }
        if let Some(author) = &mut self.author_execution {
            author.break_effect = AuthorBreakEffect::Line;
        }
    }
}
