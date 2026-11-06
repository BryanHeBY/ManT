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

//! `Nm` shares the current formatter field; only its node geometry returns.

use super::super::native_field::{FieldFlag, FieldFlags};
use super::super::{AuthorBreakEffect, InlineBuilder};

#[derive(Clone, Copy)]
pub(in crate::mandoc) struct NamePartGeometry {
    node: Option<super::DefinitionGeometryCheckpoint>,
    margin: Option<i64>,
    declared_body_origin: Option<super::state::BodyOriginProjection>,
}

impl InlineBuilder {
    pub(in crate::mandoc) fn name_part_geometry(
        &self,
        node: &libmandoc_rs::Node,
    ) -> NamePartGeometry {
        NamePartGeometry {
            node: self.definition_geometry_checkpoint(node),
            margin: self
                .execution
                .definition
                .as_ref()
                .and_then(|state| state.native_margin_units),
            declared_body_origin: self
                .execution
                .definition
                .as_ref()
                .and_then(|state| state.declared_body_origin),
        }
    }

    pub(in crate::mandoc) fn restore_name_part_geometry(&mut self, saved: NamePartGeometry) {
        self.restore_definition_geometry(saved.node);
        if let Some(state) = &mut self.execution.definition {
            state.native_margin_units = saved.margin;
            state.declared_body_origin = saved.declared_body_origin;
        }
    }

    /// `termp_nm_pre(HEAD)` ORs its flags into the live outer field. In
    /// particular an enclosing It's BRTRSP survives both Nm pre and post.
    pub(in crate::mandoc) fn enter_name_head(&mut self, effect: AuthorBreakEffect) {
        let AuthorBreakEffect::Field {
            flags: added,
            gap_cells,
            body_width_columns,
            field_width_columns,
        } = effect
        else {
            return;
        };
        self.begin_definition_head_consumption();
        let flags = self
            .native_field_parameters(false, None)
            .map_or(FieldFlags::inset(), |parameters| parameters.0)
            .combine(added, gap_cells);
        self.enter_definition_head_effect(AuthorBreakEffect::Field {
            flags,
            gap_cells,
            body_width_columns,
            field_width_columns,
        });
        let presentation = self.execution.row_presentation;
        let state = self.definition_state_mut();
        state.no_break = None;
        state.no_break_cleared = false;
        state.head_flags_cleared = false;
        state.margin_override = None;
        state.native_margin_units = Some(
            presentation
                .source_offset_units(state.field_offset_units)
                .saturating_add(i64::from(field_width_columns).saturating_mul(24)),
        );
    }

    /// The actual HEAD post clears exactly NOBREAK/BRIND/HANG and trailspace,
    /// after consuming the old field. Unlike It post it does not clear BRTRSP.
    pub(in crate::mandoc) fn finish_name_head(&mut self) -> bool {
        let occupied = self.flush_native_field();
        let flags = self
            .native_field_parameters(false, None)
            .map_or(FieldFlags::inset(), |parameters| parameters.0)
            .without(FieldFlag::NoBreak)
            .without(FieldFlag::Brind)
            .without(FieldFlag::Hang)
            .combine(FieldFlags::inset(), 0);
        let state = self.definition_state_mut();
        state.no_break = None;
        state.no_break_cleared = false;
        state.head_flags_cleared = false;
        if let Some(author) = &mut self.execution.author_execution {
            author.break_effect = AuthorBreakEffect::Field {
                flags,
                gap_cells: 0,
                body_width_columns: u16::MAX,
                field_width_columns: u16::MAX,
            };
        }
        occupied
    }

    pub(in crate::mandoc) fn enter_name_body(&mut self, width: usize) {
        self.tighten_next_boundary();
        let state = self.definition_state_mut();
        state.field_offset_units = state
            .field_offset_units
            .saturating_add(width.saturating_mul(24));
        state.hang_row.field_offset = state.field_offset_units.saturating_add(11) / 24;
        // The same BODY pre also moves the next hard row's owner-relative
        // origin. A later row commit must not overwrite an earlier accepted
        // BODY origin with the HEAD's old zero (mdoc_term.c:983-991).
        state.row.indent_columns = state
            .row
            .indent_columns
            .saturating_add(u16::try_from(width).unwrap_or(u16::MAX));
    }

    /// Unconditional `term_flushln`, shared by name and synopsis posts:
    /// consume one old-state receipt and retain its Tab reference for BODY.
    pub(in crate::mandoc) fn flush_native_field(&mut self) -> bool {
        let (start, gap, body, width, flags) = self
            .execution
            .author_execution
            .and_then(|author| match author.break_effect {
                AuthorBreakEffect::Field {
                    flags,
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                } => Some((
                    author.field_output_start,
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                )),
                AuthorBreakEffect::Line => None,
            })
            .unwrap_or((0, 0, u16::MAX, u16::MAX, FieldFlags::inset()));
        let native = self.native_field_device(false);
        let occupied = native
            .as_ref()
            .is_some_and(super::NativeFieldDevice::has_occupied_row);
        let empty_closes_row = !self.has_formatter_cell()
            && native
                .as_ref()
                .is_some_and(super::NativeFieldDevice::closes_unprinted_row);
        self.flush_captured_definition_field(
            super::flush::FieldFlush {
                start,
                gap,
                body,
                width,
                flags,
                boundary: super::flush::FieldFlushBoundary::Continue,
            },
            native,
        );
        if empty_closes_row {
            // An unconditional empty flush still reaches term.c:250-253.
            // Preserve the physical row; an owner drain cannot invent it.
            self.retain_line_breaks(1);
            self.record_completed_vertical_rows(1);
        }
        occupied
    }
}

impl super::super::InlineExecutionState {
    /// The current BODY owner already represents this resolved origin. A
    /// nested native field changes source offset, not the owner's base.
    pub(in crate::mandoc) fn enter_definition_body_origin(&mut self, absolute: usize) {
        let relative = i64::try_from(absolute)
            .unwrap_or(i64::MAX)
            .saturating_sub(i64::from(self.row_presentation.parent));
        // BODY pre follows its source NODE_LINE. That true endline can
        // consume ti and change the source offset before this width is added
        // (mdoc_term.c:314,856-861; term_ascii.c::ascii_endline). Preserve
        // that independent shift rather than reassigning an earlier origin.
        self.native_word_spacing = super::super::NativeWordSpacing::NoSpace;
        self.boundary = super::super::PendingBoundary::Tight;
        // This owner represents its structural BODY placement. Its native
        // minbl stays in hang_row; an earlier HEAD's prepaid projection may
        // not also become a leading BODY scalar.
        self.pending_field_spaces = 0;
        if let Some(state) = &mut self.definition {
            self.row_presentation.offset_shift = self
                .row_presentation
                .offset_shift
                .saturating_add(i32::try_from(relative.min(0)).unwrap_or(i32::MIN));
            state.field_offset_units = usize::try_from(relative.max(0)).unwrap_or(usize::MAX);
            state.hang_row.field_offset = state.field_offset_units.saturating_add(11) / 24;
            state.declared_body_origin = Some(super::state::BodyOriginProjection::structural(
                state.field_offset_units,
            ));
        }
    }

    /// Nm BODY's structural coordinate base survives later native breaks.
    /// Only a real still-open HEAD row needs its padding inside the combined
    /// paragraph (term.c:113-116,233-253,397-434), rather than list placement.
    pub(in crate::mandoc) fn declare_name_body_origin(&mut self) {
        let padding = if self.has_open_native_device_row() {
            super::state::BodyPaddingPlacement::Inline
        } else {
            super::state::BodyPaddingPlacement::Structural
        };
        if let Some(state) = &mut self.definition {
            state.declared_body_origin = Some(super::state::BodyOriginProjection {
                units: i64::try_from(state.field_offset_units).unwrap_or(i64::MAX),
                padding,
            });
        }
    }
}
