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
        let definition = self.definition_state_mut();
        definition.no_break = None;
        definition.no_break_cleared = false;
        definition.head_flags_cleared = true;
        if let Some(author) = &mut self.execution.author_execution {
            author.break_effect = AuthorBreakEffect::Line;
        }
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
        // HEAD post cleared trailspace, not the minbl from its actual flush.
        // Responsive BODY text does not acquire another field interpreter.
        if let Some(author) = &mut self.execution.author_execution {
            author.break_effect = AuthorBreakEffect::Line;
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

    /// A column BODY post calls `term_flushln()`, rather than `term_newln()`:
    /// pending text and \p run through the ordinary field consumer, and the
    /// Tab reference is not reset between columns (`mdoc_term.c:953`).
    pub(in crate::mandoc) fn finish_nested_column_part(&mut self) {
        if let Some((start, flags, gap)) =
            self.execution
                .author_execution
                .and_then(|author| match author.break_effect {
                    AuthorBreakEffect::Field {
                        flags, gap_cells, ..
                    } => Some((author.field_output_start, flags, gap_cells)),
                    AuthorBreakEffect::Line => None,
                })
        {
            let occupied = self.has_formatter_cell();
            self.flush_definition_field(start, gap, u16::MAX, u16::MAX, flags, false);
            if !occupied {
                // Unlike term_newln(), the column BODY post calls
                // term_flushln() even with no buffered byte. Its tail still
                // executes endline for the last column (term.c:233-253).
                self.execution.zero_advance.discard_at_row_end();
                self.retire_consumed_native_field();
                if !flags.contains(FieldFlag::NoBreak) {
                    self.retain_line_breaks(1);
                    self.definition_state_mut().hang_row.endline();
                }
            }
        }
        self.enter_nested_column_part(true);
    }

    pub(in crate::mandoc) fn restore_nested_list_geometry(&mut self, saved: NestedListScope) {
        let definition = self.definition_state_mut();
        definition.native_margin_units = saved.margin;
        definition.margin_override = saved.margin_override;
        definition.field_offset_units = saved.offset_units;
        definition.hang_row.field_offset = saved.offset_columns;
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
