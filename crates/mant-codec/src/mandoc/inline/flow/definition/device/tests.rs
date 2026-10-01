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

//! Native field device receipt regressions.

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
