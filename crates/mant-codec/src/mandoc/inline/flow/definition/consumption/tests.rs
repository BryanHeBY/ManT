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

use super::*;
use crate::mandoc::inline::flow::field_buffer::{FieldBuffer, FieldCell, FieldWrite, FillTargets};

fn operand(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::Node> {
    if node.decoder_text() == Some("\\&\\p \\p REJECTED") {
        Some(node)
    } else {
        node.children.iter().find_map(operand)
    }
}

#[test]
fn device_loop_views_are_needed_only_after_an_accepted_prefix() {
    // Complete accepted, NBRZW/p and bare-p refused sources ran fixed
    // CVS first. No actual remaining field means no loop endline at
    // term.c:217; first-pass nbr=0 likewise exits before that event.
    for (writes, expected) in [
        (FieldWrite::literal("ACCEPTED"), false),
        (vec![FieldWrite::Cell(FieldCell::ZeroWidthGraph)], false),
        (
            vec![
                FieldWrite::Cell(FieldCell::BreakMarker),
                FieldWrite::UnprojectedBlank,
                FieldWrite::Cell(FieldCell::Graph {
                    text: 'R',
                    width: 1,
                }),
            ],
            false,
        ),
        (
            vec![
                FieldWrite::Cell(FieldCell::ZeroWidthGraph),
                FieldWrite::Cell(FieldCell::BreakMarker),
                FieldWrite::UnprojectedBlank,
                FieldWrite::Cell(FieldCell::BreakMarker),
                FieldWrite::UnprojectedBlank,
                FieldWrite::Cell(FieldCell::Graph {
                    text: 'R',
                    width: 1,
                }),
            ],
            true,
        ),
    ] {
        let mut buffer = FieldBuffer::default();
        buffer.apply_writes(&writes);
        let receipt = buffer.flush_receipt(
            FillTargets {
                first: usize::MAX / 2,
                rest: usize::MAX / 2,
                unbounded: true,
            },
            false,
        );
        assert_eq!(receipt_has_authored_loop_rows(&receipt), expected);
    }
}

#[test]
fn width_passes_and_an_authored_final_tail_do_not_request_empty_loop_views() {
    use crate::mandoc::inline::flow::field_buffer::FlushReceipt;

    // Exact aa/marker counterparts ran all five pristine profiles
    // first. term_fill()299-312 distinguishes breakline from width,
    // while term_flushln()177-217 omits the accepted final pass's
    // loop endline even when that pass stopped at an authored marker.
    for word in ["aa aa aa aa", "aa aa\n ", "aa bb \n DROP"] {
        let mut buffer = FieldBuffer::default();
        buffer.apply_writes(&FieldWrite::literal(word));
        let receipt = buffer.flush_receipt(
            FillTargets {
                first: 2 * 24,
                rest: 2 * 24,
                unbounded: false,
            },
            false,
        );
        let passes = match &receipt {
            FlushReceipt::UnmodeledRewind => panic!("test word has no entry motion"),
            FlushReceipt::Accepted { passes, .. } | FlushReceipt::Rejected { passes, .. } => passes,
        };
        assert!(passes.len() > 1, "{word:?} must exercise multiple passes");
        assert!(!receipt_has_authored_loop_rows(&receipt), "{word:?}");
    }
}

#[test]
fn accepted_definition_fields_drop_only_unprinted_native_padding() {
    use crate::mandoc::formatter::AuthorFlow;

    // The exact TP TAG / nf / `A \\&` / B source ran all five
    // pristine profiles first. pre_TP/post_TP route this BODY through
    // the same term_field:389-427 deferred-blank printing rule.
    let mut builder = InlineBuilder::new();
    builder.inherit_author_execution_with_effect(
        AuthorFlow::default(),
        false,
        AuthorBreakEffect::Field {
            gap_cells: 0,
            body_width_columns: 78,
            field_width_columns: 78,
            flags: FieldFlags::inset(),
        },
    );
    builder.execution.no_fill_word_active = true;
    crate::mandoc::inline::word_input::append_normalized_word(&mut builder, "A \\&");
    assert!(!builder.discard_unprinted_definition_field_output());
    assert_eq!(mant_ir::inline_plain_text(&builder.nodes), "A");
}

#[test]
fn accepted_padding_close_follows_its_empty_word_witness() {
    use crate::mandoc::formatter::AuthorFlow;

    // The exact empty-tab-1.1 column source ran all five pristine
    // profiles first. Its five-column field accepts TAB but term_field
    // prints no padding; the tail endline still closes that empty row
    // (term.c:327-338,389-427,250-253; mdoc_term.c:946-948).
    let mut builder = InlineBuilder::new();
    builder.inherit_author_execution_with_effect(
        AuthorFlow::default(),
        false,
        AuthorBreakEffect::Field {
            gap_cells: 1,
            body_width_columns: 5,
            field_width_columns: 5,
            flags: FieldFlags::column(false),
        },
    );
    crate::mandoc::inline::word_input::append_normalized_word(&mut builder, "\t");
    builder.hard_break();
    assert_eq!(mant_ir::inline_plain_text(&builder.nodes), "\n");
    assert_eq!(builder.completed_empty_rows(), 1);
    assert!(builder.closed_graph_tail().is_closed());
}

#[test]
fn ordinary_width_passes_never_build_the_empty_loop_device_view() {
    use crate::mandoc::formatter::AuthorFlow;
    use crate::mandoc::inline::{AuthorBreakEffect, InlineBuilder};

    // The complete 1024/2048/4096/8192-word HEAD sources ran pristine
    // first. This bounded inset field exercises term_fill's width stop
    // (term.c:299-301) without changing the native input or device tail.
    for words in [1024, 2048, 4096, 8192] {
        let mut builder = InlineBuilder::new();
        builder.inherit_author_execution_with_effect(
            AuthorFlow::default(),
            false,
            AuthorBreakEffect::Field {
                gap_cells: 0,
                body_width_columns: 4,
                field_width_columns: 4,
                flags: FieldFlags::inset(),
            },
        );
        builder.append_text(&"aa ".repeat(words));
        EMPTY_LOOP_DEVICE_VIEWS.with(|views| views.set(0));
        builder.discard_unprinted_definition_field_output();
        assert_eq!(EMPTY_LOOP_DEVICE_VIEWS.with(std::cell::Cell::get), 0);
    }

    let mut builder = InlineBuilder::new();
    builder.inherit_author_execution_with_effect(
        AuthorFlow::default(),
        false,
        AuthorBreakEffect::Field {
            gap_cells: 0,
            body_width_columns: 4,
            field_width_columns: 4,
            flags: FieldFlags::inset(),
        },
    );
    let source = b".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.ll 2n\n.No \"\\&\\p \\p REJECTED\"\n.No AFTER\n";
    let parsed = libmandoc_rs::Parser::default()
        .parse_bytes("native-loop-view.1", source)
        .unwrap();
    crate::mandoc::inline::append_text_node(&mut builder, operand(&parsed.document.root).unwrap());
    EMPTY_LOOP_DEVICE_VIEWS.with(|views| views.set(0));
    builder.discard_unprinted_definition_field_output();
    assert_eq!(EMPTY_LOOP_DEVICE_VIEWS.with(std::cell::Cell::get), 1);
}
