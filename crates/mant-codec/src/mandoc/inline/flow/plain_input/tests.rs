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

use super::projection::find_literal_tail_row;
use super::*;

#[test]
fn plain_tail_outcomes_distinguish_acceptance_blanks_and_rejection() {
    use super::super::field_buffer::{FieldCell, FieldWrite};
    for inherited in [false, true] {
        for (writes, rejected, unprinted) in [
            (
                vec![FieldWrite::Cell(FieldCell::ZeroWidthGraph)],
                false,
                true,
            ),
            (
                vec![FieldWrite::Cell(FieldCell::BreakableBlank)],
                false,
                true,
            ),
            (FieldWrite::literal("X"), false, false),
            (
                vec![
                    FieldWrite::Cell(FieldCell::BreakMarker),
                    FieldWrite::Cell(FieldCell::BreakableBlank),
                    FieldWrite::Cell(FieldCell::ZeroWidthGraph),
                ],
                true,
                false,
            ),
        ] {
            let mut builder = InlineBuilder::new();
            if inherited {
                builder.execution.detached_device_row = Some(super::super::DetachedDeviceRow {
                    viscol: 1,
                    minbl: 0,
                    page_origin_printed: true,
                });
            }
            builder.execution.plain_input.buffer.apply_writes(&writes);
            let outcome = builder.retire_plain_flush_unit_with_tail();
            assert_eq!(outcome.rejected(), rejected);
            assert_eq!(outcome.closes_unprinted_row(), unprinted && !inherited);
            assert_eq!(outcome.closes_represented_row(), unprinted && inherited);
        }
    }
}

#[test]
fn only_a_real_plain_close_commits_its_unprinted_tail_once() {
    use super::super::field_buffer::{FieldCell, FieldWrite};
    // The exact 2/3/4 Ft controls ran all five native profiles first.
    // An owner drain can retain an empty-word witness but cannot create
    // the real term_flushln tail endline (term.c:233-253).
    for cell in [FieldCell::ZeroWidthGraph, FieldCell::BreakableBlank] {
        let mut projected = InlineBuilder::new();
        projected
            .execution
            .plain_input
            .buffer
            .apply_writes(&[FieldWrite::Cell(cell.clone())]);
        let (_, _, rows, _) = projected.take_paragraph_segment(false);
        assert_eq!(rows, 0);

        // The block .sp/.br path consumes this same unit at its real
        // line-request boundary. Both a NBRZW cell and buffered ordinary
        // blanks reach term_flushln's tail endline (term.c:143-146,250).
        let mut requested = InlineBuilder::new();
        requested
            .execution
            .plain_input
            .buffer
            .apply_writes(&[FieldWrite::Cell(cell.clone())]);
        let (_, _, rows, _) = requested.take_paragraph_segment(true);
        assert_eq!(rows, 1);
        let (_, _, rows, _) = requested.take_paragraph_segment(false);
        assert_eq!(rows, 0);

        let mut consumed = InlineBuilder::new();
        consumed
            .execution
            .plain_input
            .buffer
            .apply_writes(&[FieldWrite::Cell(cell)]);
        consumed.execute_native_newline();
        assert_eq!(consumed.execution.completed_vertical_rows, 1);
        let (_, _, rows, _) = consumed.take_paragraph_segment(false);
        assert_eq!(rows, 1);
        let (_, _, rows, _) = consumed.take_paragraph_segment(false);
        assert_eq!(rows, 0);
    }
}

#[test]
fn a_requested_plain_tail_clears_backafter_only_when_it_flushed_cells() {
    use super::super::field_buffer::{FieldCell, FieldWrite};
    // The full X/Z output-carrier sources ran pristine first. Their
    // ignored escape writes NBRZW without consuming BACKAFTER, but the
    // real paragraph request flush then clears it (term.c:233-237).
    // A cell-less bare-z has no term_newln flush (term.c:475-481).
    for line_request in [false, true] {
        for buffered in [false, true] {
            let mut builder = InlineBuilder::new();
            builder.execution.zero_advance.inherit_armed(true);
            if buffered {
                builder
                    .execution
                    .plain_input
                    .buffer
                    .apply_writes(&[FieldWrite::Cell(FieldCell::ZeroWidthGraph)]);
            }
            let (_, _, rows, _) = builder.take_paragraph_segment(line_request);
            let flushed = line_request && buffered;
            assert_eq!(rows, u16::from(flushed));
            assert_eq!(builder.execution.zero_advance.take_armed(), !flushed);
        }
    }
}

#[test]
fn represented_plain_tails_notify_the_existing_owner_only_at_a_real_close() {
    use super::super::field_buffer::{FieldCell, FieldWrite};
    // The exact repeated Ft blank/NBRZW sources ran pristine first.
    // term_newln closes the already printed tag row before term_vspace
    // emits its own new row (term.c:475-497). A blank's projection flag
    // cannot conceal that close from the existing BODY observer.
    for blank in [false, true] {
        for inherited in [false, true] {
            for actual_close in [false, true] {
                let mut builder = InlineBuilder::new();
                if inherited {
                    builder.execution.detached_device_row = Some(super::super::DetachedDeviceRow {
                        viscol: 1,
                        minbl: 0,
                        page_origin_printed: true,
                    });
                }
                builder.execution.has_printable_content = blank;
                builder.execution.formatter_column = super::super::FormatterColumn::Advanced;
                builder
                    .execution
                    .plain_input
                    .buffer
                    .apply_writes(&[FieldWrite::Cell(if blank {
                        FieldCell::BreakableBlank
                    } else {
                        FieldCell::ZeroWidthGraph
                    })]);
                if actual_close {
                    builder.execute_native_newline();
                } else {
                    builder.take_paragraph_segment(false);
                }
                assert_eq!(
                    builder.execution.take_leading_line_boundary().is_some(),
                    actual_close && (!blank || inherited)
                );
                assert_eq!(
                    builder.execution.completed_vertical_rows,
                    u16::from(actual_close && !inherited)
                );
            }
        }
    }
}

#[test]
fn empty_loop_classification_scans_each_accepted_cell_once() {
    use super::super::field_buffer::{
        FieldBuffer, FieldCell, FieldWrite, FillTargets, FlushReceipt,
    };

    // All complete NBRZW/p/refused-tail sources ran the five pristine
    // profiles before this counter. term_field() visits accepted native
    // intervals forward once (term.c:389-444), even when its zero-width
    // cells do not print and a previously printed row remains occupied.
    for count in [1, 2, 64, 1024, 2048, 4096, 8192] {
        for (buffered_prefix, inherited_row) in [(false, false), (true, false), (false, true)] {
            let mut buffer = FieldBuffer::default();
            let mut writes = if buffered_prefix {
                FieldWrite::literal("BEFORE")
            } else {
                Vec::new()
            };
            for _ in 0..count {
                writes.extend([
                    FieldWrite::Cell(FieldCell::ZeroWidthGraph),
                    FieldWrite::Cell(FieldCell::BreakMarker),
                    FieldWrite::UnprojectedBlank,
                ]);
            }
            writes.push(FieldWrite::Cell(FieldCell::BreakMarker));
            writes.push(FieldWrite::UnprojectedBlank);
            writes.extend(FieldWrite::literal("REJECTED"));
            buffer.apply_writes(&writes);
            let receipt = buffer.flush_receipt(
                FillTargets {
                    first: usize::MAX / 2,
                    rest: usize::MAX / 2,
                    unbounded: true,
                },
                false,
            );
            let FlushReceipt::Rejected { passes, .. } = receipt else {
                panic!("a graphless leading marker rejects the suffix");
            };
            assert_eq!(passes.len(), count);
            let mut visits = 0;
            let rows = completed_empty_plain_passes(
                &buffer,
                &passes,
                true,
                inherited_row,
                |start, end| {
                    visits += end.saturating_sub(start);
                    buffer.printed_columns(start, end, 0, 0).is_some()
                },
            );
            assert_eq!(
                usize::from(rows),
                count - usize::from(buffered_prefix || inherited_row)
            );
            assert_eq!(visits, count * 2 + usize::from(buffered_prefix) * 6);
            assert!(
                visits <= buffer.cells().len(),
                "{count} passes revisited {visits} cells"
            );
        }
    }
}

#[test]
fn accepted_single_pass_never_scans_for_completed_loop_rows() {
    use super::super::field_buffer::{
        FieldBuffer, FieldCell, FieldWrite, FillTargets, FlushReceipt,
    };

    // Exact ACCEPTED and NBRZW-only words ran pristine first. A lone
    // accepted pass exits before endline (term.c:200-217); its empty
    // final row remains handled by retain_accepted_unit_output().
    for writes in [
        FieldWrite::literal("ACCEPTED"),
        vec![FieldWrite::Cell(FieldCell::ZeroWidthGraph)],
    ] {
        let mut buffer = FieldBuffer::default();
        buffer.apply_writes(&writes);
        let FlushReceipt::Accepted { passes, .. } = buffer.flush_receipt(
            FillTargets {
                first: usize::MAX / 2,
                rest: usize::MAX / 2,
                unbounded: true,
            },
            false,
        ) else {
            panic!("one graphful pass is accepted");
        };
        assert_eq!(passes.len(), 1);
        assert_eq!(
            completed_empty_plain_passes(&buffer, &passes, false, false, |_, _| {
                panic!("there is no loop row to scan")
            }),
            0
        );
    }
}

#[test]
fn literal_tail_queries_stop_before_completed_row_history() {
    for count in [64, 1024, 4096] {
        let mut nodes = Vec::new();
        for _ in 0..count {
            nodes.push(Inline::Text {
                value: String::new(),
            });
            nodes.push(Inline::line_break());
        }
        let mut visits = 0;
        assert_eq!(
            find_literal_tail_row(&nodes, &mut || visits += 1),
            Some(false)
        );
        assert_eq!(visits, 1);
        nodes.push(Inline::Strong {
            children: vec![Inline::Text {
                value: String::new(),
            }],
        });
        nodes.push(Inline::anchor("identity"));
        visits = 0;
        assert_eq!(
            find_literal_tail_row(&nodes, &mut || visits += 1),
            Some(true)
        );
        assert_eq!(visits, 3);
    }
}
