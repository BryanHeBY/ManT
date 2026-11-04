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

//! Fn/Fo share the real four-column name field and its argument flush.

use super::super::native_field::{FieldFlag, FieldFlags};
use super::super::{AuthorBreakEffect, InlineBuilder};

impl InlineBuilder {
    /// `mdoc_term.c::termp_fn_pre` and `termp_fo_pre(BODY)` temporarily set
    /// rmargin=offset+4 and OR these flags; neither replaces trailspace.
    pub(in crate::mandoc) fn with_function_name_field(&mut self, append: impl FnOnce(&mut Self)) {
        if self.execution.author_execution.is_none() {
            self.inherit_author_execution(crate::mandoc::formatter::AuthorFlow::Automatic, false);
        }
        self.begin_definition_head_consumption();
        let old_effect = self
            .execution
            .author_execution
            .expect("author")
            .break_effect;
        let (old_flags, body, width, gap) = match old_effect {
            AuthorBreakEffect::Field {
                flags,
                body_width_columns,
                field_width_columns,
                gap_cells,
            } => (flags, body_width_columns, field_width_columns, gap_cells),
            AuthorBreakEffect::Line => (FieldFlags::inset(), u16::MAX, u16::MAX, 0),
        };
        let flags = old_flags.combine(
            FieldFlags::new(
                &[FieldFlag::NoBreak, FieldFlag::Brind, FieldFlag::Hang],
                gap,
            ),
            gap,
        );
        self.execution
            .author_execution
            .as_mut()
            .expect("author")
            .break_effect = AuthorBreakEffect::Field {
            flags,
            body_width_columns: body,
            field_width_columns: 4,
            gap_cells: gap,
        };
        let presentation = self.execution.row_presentation;
        let state = self.definition_state_mut();
        let saved_margin = state.native_margin_units;
        let saved_override = state.margin_override;
        state.native_margin_units = Some(
            presentation
                .source_offset_units(state.field_offset_units)
                .saturating_add(4 * 24),
        );
        state.margin_override = None;
        state.no_break = None;
        state.no_break_cleared = false;
        state.head_flags_cleared = false;
        append(self);
        self.flush_native_field();
        let flags = flags
            .without(FieldFlag::NoBreak)
            .without(FieldFlag::Brind)
            .without(FieldFlag::Hang);
        self.execution
            .author_execution
            .as_mut()
            .expect("author")
            .break_effect = AuthorBreakEffect::Field {
            flags,
            body_width_columns: body,
            field_width_columns: width,
            gap_cells: gap,
        };
        let state = self.definition_state_mut();
        state.native_margin_units = saved_margin;
        state.margin_override = saved_override;
        state.no_pad = true;
        state.field_offset_units = state.field_offset_units.saturating_add(4 * 24);
        state.hang_row.field_offset = state.field_offset_units.saturating_add(11) / 24;
    }

    /// Fo emits '(' before its true BODY-pre name-field flush. The shared
    /// native scope survives structural child-owner drains until BODY post.
    pub(in crate::mandoc) fn begin_function_body(&mut self, pretty: bool) {
        if pretty {
            self.execution.enter_native_field_scope();
            self.with_function_name_field(|builder| {
                builder.tighten_next_boundary();
                builder.append_text("(");
            });
        } else {
            self.tighten_next_boundary();
            self.append_text("(");
        }
        self.tighten_next_boundary();
    }

    /// Font pop precedes this post. Explicit Fc consumes the same BODY
    /// registration, so its later AST return cannot repeat punctuation/flush.
    pub(in crate::mandoc) fn finish_function_body(&mut self, pretty: bool) {
        self.tighten_next_boundary();
        self.append_text(if pretty { ");" } else { ")" });
        if pretty {
            self.flush_native_field();
            self.execution.exit_native_field_scope();
        }
    }

    /// term.c:113-116: NOPAD suppresses offset padding for only the first
    /// field pass, but minbl still applies. Loop restarts use normal offset.
    pub(in crate::mandoc::inline::flow::definition) fn native_first_field_origin(&self) -> i64 {
        let state = self.execution.definition.as_ref().expect("field session");
        if !state.no_pad {
            return i64::try_from(state.field_offset_units).unwrap_or(i64::MAX);
        }
        let presentation = self.execution.row_presentation;
        let position = if state.hang_row.native_row_occupied() {
            presentation.device_column(state.hang_row.viscol)
        } else {
            -i64::from(presentation.parent)
        };
        position.saturating_sub(i64::from(presentation.offset_shift))
    }
}
