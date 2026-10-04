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

use super::{FieldBuffer, FieldCell};

#[test]
fn final_scan_copies_native_facts_without_output_ownership_history() {
    let mut source = FieldBuffer::default();
    for _ in 0..8192 {
        source.push_graph('中', 2);
    }
    source.push_non_breaking_blank();
    source.push_separator_blank();
    source.push_break_marker();
    let pass = source.fill_pass(32).unwrap();
    source.commit_pass(pass, 32 * super::EN);
    let scan = source.native_flush_scan();
    assert_eq!(scan.cells, source.cells);
    assert_eq!(scan.resume, source.resume);
    assert_eq!(scan.tab_offset, source.tab_offset);
    assert_eq!(scan.pass_tab_offset, source.pass_tab_offset);
    assert_eq!(scan.last_graph_position, source.last_graph_position);
    assert_eq!(scan.nonbreaking_positions, source.nonbreaking_positions);
    assert_eq!(scan.blank_positions, source.blank_positions);
    assert!(std::sync::Arc::ptr_eq(&scan.tabs, &source.tabs));
    assert_eq!(scan.projection_prefix.capacity(), 0);
    assert!(scan.projected_pass_ends.is_empty());
    assert_eq!(scan.committed_passes.capacity(), 0);
    assert!(scan.scan.is_none());
    assert_eq!(source.projection_prefix.len(), source.cells.len() + 1);
    assert!(source.projected_pass_ends.contains(&pass.end));
    assert_eq!(source.committed_passes.len(), 1);
}

/// The native tail sweep skips markers, tabs and NBRZW, while direct NBRSP
/// remains significant until `term_fill()` rewrites it (term.c:177-198/340-347).
/// Check the compact metadata against that full sweep after every mutation,
/// including `encode1()`'s retreat over a normalized blank (term.c:901-908).
#[test]
fn tail_summary_agrees_with_native_cells_after_normalization_and_retreat() {
    fn check(buffer: &FieldBuffer) {
        for start in 0..=buffer.cells().len() {
            for trailing_blanks in [false, true] {
                let native_tail = buffer.cells()[start..].iter().any(|cell| match cell {
                    FieldCell::BreakableBlank => trailing_blanks,
                    FieldCell::BreakMarker
                    | FieldCell::Tab
                    | FieldCell::TabReference
                    | FieldCell::ZeroWidthGraph
                    | FieldCell::Breakpoint => false,
                    FieldCell::Graph { .. }
                    | FieldCell::Hyphen
                    | FieldCell::NonBreakingBlank
                    | FieldCell::Backline => true,
                });
                assert_eq!(
                    buffer.has_non_ignorable_after(start, trailing_blanks),
                    native_tail,
                    "start={start} BRTRSP={trailing_blanks} cells={:?}",
                    buffer.cells()
                );
            }
        }
    }

    let cells = [
        FieldCell::Graph {
            text: '中',
            width: 2,
        },
        FieldCell::Hyphen,
        FieldCell::NonBreakingBlank,
        FieldCell::BreakableBlank,
        FieldCell::BreakMarker,
        FieldCell::Tab,
        FieldCell::ZeroWidthGraph,
        FieldCell::Backline,
    ];
    for mut variant in 0..cells.len().pow(4) {
        let mut buffer = FieldBuffer::default();
        for _ in 0..4 {
            buffer.apply_writes(&[super::FieldWrite::Cell(
                cells[variant % cells.len()].clone(),
            )]);
            variant /= cells.len();
            check(&buffer);
        }
        for through in 0..=buffer.cells().len() {
            let mut normalized = buffer.clone();
            normalized.normalize_scanned_cells(through);
            check(&normalized);
            normalized.arm_backbefore();
            normalized.push_graph('Z', 1);
            check(&normalized);
            normalized.clear_consumed_field();
            check(&normalized);
            normalized.push_non_breaking_blank();
            check(&normalized);
            normalized.normalize_scanned_cells(1);
            check(&normalized);
        }
    }
}

/// The tail query can inspect the last ordinary glyph without retaining an
/// ordered-tree entry for each one. Long suffixes must also retain exact tail
/// classification after appending and normalizing direct native KEEP blanks.
#[test]
fn long_graph_tail_survives_later_fixed_blank_normalization() {
    let mut buffer = FieldBuffer::default();
    for _ in 0..8192 {
        buffer.push_graph('x', 1);
    }
    buffer.push_non_breaking_blank();
    buffer.push_separator_blank();
    assert!(buffer.has_non_ignorable_after(8192, false));
    buffer.normalize_scanned_cells(buffer.cells().len());
    assert!(buffer.has_non_ignorable_after(8191, false));
    assert!(!buffer.has_non_ignorable_after(8192, false));
    assert!(buffer.has_non_ignorable_after(8192, true));
    buffer.arm_backbefore();
    buffer.push_graph('Y', 1);
    assert!(buffer.has_non_ignorable_after(8193, false));
    buffer.clear();
    assert!(!buffer.has_non_ignorable_after(0, true));
}

/// (a) of the fixed-CVS oracle pair: `\zX\p` + `\p Y` keeps Y. The
/// retreat eats the blank before Y, so pass two resumes at the second
/// marker and accepts Y itself (term.c:901-908 with 263-367; pass one
/// accepts `X` + marker, nbr = the breaking blank's index).
#[test]
fn retreat_blank_keeps_the_following_graph() {
    let mut buffer = FieldBuffer::default();
    buffer.push_graph('X', 1);
    buffer.arm_backbefore();
    buffer.push_break_marker(); // \p of word one
    buffer.push_separator_blank(); // word two's separator
    buffer.push_break_marker(); // \p of word two
    buffer.push_separator_blank(); // " Y" operand blank
    buffer.push_graph('Y', 1); // retreat eats the operand blank
    let first = buffer
        .fill_pass(usize::MAX / 2)
        .expect("pass one accepts X");
    assert_eq!(first.end, 2, "X + marker; blank breaks");
    buffer.advance_past(first.end);
    buffer.consume_break_blanks();
    let second = buffer
        .fill_pass(usize::MAX / 2)
        .expect("pass two accepts Y");
    assert!(second.end >= 4);
    assert_eq!(second.width, 1);
}

/// (b) of the pair: the empty operand's real blank survives after the
/// marker, so the next pass stops graphless at it (nbr stays 0 through
/// term.c:362-366) and the remainder is wiped (143-146 with 233-237).
#[test]
fn blank_after_marker_rejects_and_wipes() {
    let mut buffer = FieldBuffer::default();
    buffer.push_graph('X', 1);
    buffer.arm_backbefore();
    buffer.push_break_marker(); // word one's \p
    buffer.push_separator_blank(); // word two's separator
    buffer.push_break_marker(); // word two's \p
    buffer.push_separator_blank(); // `.No ""` term_word("") blank
    buffer.push_separator_blank(); // Y's own word separator
    buffer.push_graph('Y', 1); // retreat eats only Y's own blank
    let first = buffer
        .fill_pass(usize::MAX / 2)
        .expect("pass one accepts X");
    buffer.advance_past(first.end);
    buffer.consume_break_blanks();
    assert!(
        buffer.fill_pass(usize::MAX / 2).is_none(),
        "the surviving blank under breakline rejects the pass"
    );
    buffer.wipe_remainder();
    assert!(buffer.is_empty());
}

/// An armed marker with no graph before the stop also rejects: `\p`
/// alone before a separator blank never prints (term.c:294 with
/// 362-366 leaving nbr at 0).
#[test]
fn armed_marker_without_graph_rejects() {
    let mut buffer = FieldBuffer::default();
    buffer.push_break_marker();
    buffer.push_separator_blank();
    buffer.push_graph('X', 1);
    assert!(buffer.fill_pass(usize::MAX / 2).is_none());
}

/// term.c:350-351: a graph that overruns the target returns at the
/// last recorded blank; the blank itself is consumed with the break
/// (205-207), so the next pass starts at the overflowing word.
#[test]
fn vtarget_overrun_breaks_at_the_last_recorded_blank() {
    let mut buffer = FieldBuffer::default();
    buffer.push_graph('a', 1);
    buffer.push_separator_blank();
    buffer.push_graph('b', 1);
    buffer.push_separator_blank();
    buffer.push_graph('c', 1);
    let pass = buffer.fill_pass(2).expect("breaks inside the slice");
    assert_eq!(pass.end, 1, "accepts `a`; blank consumed");
    buffer.advance_past(pass.end);
    buffer.consume_break_blanks();
    assert_eq!(buffer.resume_offset(), 2, "next pass starts at `b`");
}

/// term.c:362-366: a word running to the buffer end is accepted whole,
/// even past the target (nbr==0 has no recorded candidate to return
/// to; the 350-351 guard only fires when one exists).
#[test]
fn trailing_word_is_accepted_whole() {
    let mut buffer = FieldBuffer::default();
    buffer.push_separator_blank();
    buffer.push_graph('a', 1);
    buffer.push_graph('b', 1);
    let pass = buffer.fill_pass(1).expect("whole word accepted");
    assert_eq!(pass.end, 3);
    assert_eq!(pass.width, 3, "the leading blank counts width");
}

/// A non-breaking blank counts width and never breaks
/// (`ASCII_NBRSP` through the default branch, term.c:342-347).
#[test]
fn non_breaking_blank_counts_width_without_breaking() {
    let mut buffer = FieldBuffer::default();
    buffer.push_graph('a', 1);
    buffer.push_non_breaking_blank();
    buffer.push_graph('b', 1);
    let pass = buffer.fill_pass(1).expect("nbr does not stop the pass");
    assert_eq!(pass.end, 3);
    assert_eq!(pass.width, 3);
}

/// term.c:283-286 with 906: a BACKBEFORE retreat over a graph buffers
/// `'\b'`; fill subtracts the predecessor's width before the new graph
/// adds its own, so a `\z` overstrike keeps one column.
#[test]
fn backline_retreat_subtracts_the_previous_width() {
    let mut buffer = FieldBuffer::default();
    buffer.push_graph('X', 1);
    buffer.arm_backbefore();
    buffer.push_graph('Y', 1); // buffers '\b' over X
    assert_eq!(
        buffer.cells(),
        &[
            super::FieldCell::Graph {
                text: 'X',
                width: 1
            },
            super::FieldCell::Backline,
            super::FieldCell::Graph {
                text: 'Y',
                width: 1
            },
        ][..]
    );
    let pass = buffer.fill_pass(usize::MAX / 2).expect("accepted");
    assert_eq!(pass.width, 1, "overstrike keeps one column");
}
#[test]
fn source_cells_and_projection_ownership_are_independent() {
    use super::{FieldCell, FieldWrite};
    // Exact pristine fixture overstrike_projection.roff renders C. The
    // native A/B cells stay graphs (encode1), while their zero-advance
    // projections are replaced. Neither fact can be inferred from IR.
    let mut buffer = FieldBuffer::default();
    buffer.apply_writes(&[
        FieldWrite::ArmBackafter,
        FieldWrite::Cell(FieldCell::Graph {
            text: 'A',
            width: 1,
        }),
        FieldWrite::ArmBackafter,
        FieldWrite::Cell(FieldCell::Graph {
            text: 'B',
            width: 1,
        }),
        FieldWrite::Cell(FieldCell::Graph {
            text: 'C',
            width: 1,
        }),
    ]);
    assert_eq!(
        buffer
            .cells()
            .iter()
            .filter(|cell| matches!(cell, FieldCell::Graph { .. }))
            .count(),
        3
    );
    assert_eq!(buffer.projection_length(0, buffer.cells().len()), 1);
    assert!(buffer.fill_pass(usize::MAX / 2).is_some());
}

#[test]
fn unicode_spaces_and_zero_width_scalars_use_encode1() {
    // Exact pristine fixed-space-native and zero-unicode-native inputs
    // were run before this assertion. term_word()629 and 817 call
    // encode1() for Unicode glyphs; ASCII_NBRSP/NBRZW controls are
    // separate bufferc() writes even when their visible widths agree.
    for scalar in ['\u{a0}', '\u{200b}'] {
        let mut buffer = FieldBuffer::default();
        buffer.apply_writes(&[super::FieldWrite::ArmBackafter]);
        buffer.apply_writes(&super::FieldWrite::literal(&scalar.to_string()));
        assert!(buffer.backbefore_armed);
        assert!(!buffer.backafter_armed);
        assert!(
            matches!(buffer.cells()[0], super::FieldCell::Graph { text, .. } if text == scalar)
        );
    }
}

#[test]
fn detached_head_blank_executes_without_reassigning_its_projection() {
    // Exact pristine runin-head-pending/plain cases execute one generated
    // Unicode escaped space regardless of the surviving semantic gap.
    let mut buffer = FieldBuffer::default();
    buffer.apply_writes(&[super::FieldWrite::ArmBackafter]);
    buffer.apply_writes(&super::FieldWrite::literal("X"));
    buffer.detach_projection_owner();
    let before = buffer.cells().len();
    buffer.apply_writes(&[super::FieldWrite::OwnedBlank { projected: false }]);
    assert!(matches!(buffer.cells()[before], super::FieldCell::Backline));
    assert!(matches!(
        buffer.cells()[before + 1],
        super::FieldCell::Graph { text: '\u{a0}', .. }
    ));
    assert_eq!(buffer.projection_length(0, before), 1, "HEAD keeps X");
    assert_eq!(buffer.projection_length(before, buffer.cells().len()), 0);
    assert!(!buffer.backbefore_armed);
}

#[test]
fn growing_unbroken_suffix_is_scanned_incrementally() {
    let mut buffer = FieldBuffer::default();
    for _ in 0..16_384 {
        buffer.push_graph('a', 1);
        let _ = buffer.fill_pass(usize::MAX / 2);
    }
    assert!(buffer.scan_work <= buffer.cells().len() * 2);
}

#[test]
fn many_passes_in_one_operand_do_not_rescan_projection_prefixes() {
    // Exact pristine single_operand_many_passes.roff keeps one row per
    // aa\p bb\p segment. This scale test counts source-cell work rather
    // than wall time, including the native-to-projection prefix index.
    let mut buffer = FieldBuffer::default();
    for _ in 0..8192 {
        buffer.push_graph('a', 1);
        buffer.push_graph('a', 1);
        buffer.push_break_marker();
        buffer.push_separator_blank();
    }
    buffer.push_graph('a', 1);
    let mut count = 0;
    while let Some(pass) = buffer.fill_pass(usize::MAX / 2) {
        let _ = buffer.projection_length(0, pass.end);
        count += 1;
        if !buffer.has_non_ignorable_after(pass.end, false) {
            break;
        }
        buffer.commit_pass(pass, usize::MAX / 2);
    }
    assert_eq!(count, 8193);
    assert!(buffer.scan_work <= buffer.cells().len() * 2);
    assert_eq!(
        buffer.projection_work, 0,
        "prefix lookups never walk earlier cells"
    );
}

#[test]
fn ignored_tail_after_a_pending_pass_uses_cached_native_presence() {
    let mut buffer = FieldBuffer::default();
    buffer.push_graph('X', 1);
    buffer.push_break_marker();
    for _ in 0..16_384 {
        buffer.push_separator_blank();
        buffer.push_break_marker();
        let pass = buffer.fill_pass(usize::MAX / 2).expect("X prefix");
        assert!(!buffer.has_non_ignorable_after(pass.end, false));
    }
    assert!(buffer.scan_work <= buffer.cells().len() * 2);
}

#[test]
fn recovery_cell_preserves_native_flags_and_separate_projection_width() {
    // Exact \z\[unknownname]YZ fixture first verified with pristine
    // CVS: raw output Y\bZ. term.c:620-638 buffers ASCII_NBRZW directly,
    // so it cannot consume BACKAFTER; encode1(Y) does, then Z replaces Y.
    let mut buffer = FieldBuffer::default();
    buffer.apply_writes(&[
        super::FieldWrite::ArmBackafter,
        super::FieldWrite::RecoveryGlyph {
            projected_scalars: 0,
        },
    ]);
    buffer.apply_writes(&super::FieldWrite::literal("YZ"));
    assert_eq!(
        buffer.cells,
        [
            FieldCell::ZeroWidthGraph,
            FieldCell::Graph {
                text: 'Y',
                width: 1
            },
            FieldCell::Backline,
            FieldCell::Graph {
                text: 'Z',
                width: 1
            }
        ]
    );
    assert_eq!(buffer.projection_length(0, buffer.cells.len()), 1);

    // Ordinary unknown spelling is retained by semantic recovery only:
    // a zero-width native graph still owns its entire visible spelling.
    let mut buffer = FieldBuffer::default();
    buffer.apply_writes(&[super::FieldWrite::RecoveryGlyph {
        projected_scalars: 14,
    }]);
    let pass = buffer.fill_pass(1).expect("native zero-width graph");
    assert_eq!(pass.width, 0);
    assert_eq!(buffer.projection_length(0, pass.end), 14);
}

#[test]
fn configuration_restart_keeps_projected_pass_identity_at_earlier_offsets() {
    // A configuration change invalidates scan position, not row events
    // already delivered to the output owner. Exercise the private lifecycle
    // directly: the next legal pass can precede the earlier accepted end.
    let mut buffer = FieldBuffer::default();
    buffer.apply_writes(&super::FieldWrite::literal("AAA BBB CCC"));
    let later = buffer.fill_pass(8).unwrap();
    buffer.commit_pass(later, 8 * 24);
    assert!(
        buffer.configure_tabs(&std::sync::Arc::new(super::TabStops::from_arguments(
            ["2n"].into_iter()
        )))
    );
    let earlier = buffer.fill_pass(4).unwrap();
    assert!(earlier.end < later.end);
    buffer.commit_pass(earlier, 4 * 24);
    assert!(buffer.has_projected_pass(later.end));
    assert!(buffer.has_projected_pass(earlier.end));
    buffer.clear_consumed_field();
    assert!(!buffer.has_projected_rows());
}

#[test]
fn native_hyphen_lookahead_is_normalized_before_the_next_pass() {
    // term.c:316 mutates ASCII_HYPH before deciding whether this pass can
    // accept it. Exercise that buffer primitive, including a suffix that
    // was scanned but not printed; ordinary adjacent source '-' operands
    // do not create this internal cell topology (roff.c:1876-1905).
    let mut buffer = FieldBuffer::default();
    buffer.apply_writes(&[
        super::FieldWrite::Cell(FieldCell::Graph {
            text: 'a',
            width: 1,
        }),
        super::FieldWrite::Cell(FieldCell::Hyphen),
        super::FieldWrite::Cell(FieldCell::Hyphen),
        super::FieldWrite::Cell(FieldCell::Graph {
            text: 'b',
            width: 1,
        }),
        super::FieldWrite::Cell(FieldCell::Graph {
            text: 'c',
            width: 1,
        }),
    ]);
    let first = buffer.fill_pass(2).unwrap();
    assert_eq!((first.end, first.width), (2, 2));
    assert!(matches!(
        buffer.cells()[2],
        FieldCell::Graph {
            text: '-',
            width: 1
        }
    ));
    buffer.advance_past(first.end);
    let second = buffer.fill_pass(2).unwrap();
    assert_eq!((second.end, second.width), (5, 3));
    assert_eq!(second.boundary, super::FillBoundary::BufferEnd);
}

#[test]
fn provisional_hyphen_scans_do_not_change_the_native_input() {
    // Width predictions are not term_fill() execution; a changed .ta or
    // restored scope can still select the real flush target. Only a
    // committed marker pass or the owned flush receipt normalizes cells.
    let mut buffer = FieldBuffer::default();
    buffer.apply_writes(&[
        super::FieldWrite::Cell(FieldCell::Graph {
            text: 'a',
            width: 1,
        }),
        super::FieldWrite::Cell(FieldCell::Hyphen),
        super::FieldWrite::Cell(FieldCell::Hyphen),
        super::FieldWrite::Cell(FieldCell::Graph {
            text: 'b',
            width: 1,
        }),
        super::FieldWrite::Cell(FieldCell::Graph {
            text: 'c',
            width: 1,
        }),
    ]);
    let first = buffer.fill_pass_units(2 * 24).unwrap();
    assert!(matches!(buffer.cells()[2], FieldCell::Hyphen));
    buffer.commit_pass(first, 2 * 24);
    assert!(matches!(
        buffer.cells()[2],
        FieldCell::Graph {
            text: '-',
            width: 1
        }
    ));
    let second = buffer.fill_pass_units(2 * 24).unwrap();
    assert_eq!((second.end, second.width), (5, 3));
}

#[test]
fn kept_blank_lookahead_normalizes_before_resume_and_tail_sweep() {
    // term.c:340 writes ordinary SP before the overflow guard. Like a
    // scanned hyphen, this byte may lie beyond nbr and still changes the
    // next pass's consumed blanks (term.c:205-207). Bk source probes ran the
    // pristine oracle before this internal buffer-primitive assertion.
    let mut buffer = FieldBuffer::default();
    buffer.apply_writes(&[
        super::FieldWrite::Cell(FieldCell::Graph {
            text: 'a',
            width: 1,
        }),
        super::FieldWrite::Cell(FieldCell::BreakableBlank),
        super::FieldWrite::Cell(FieldCell::NonBreakingBlank),
        super::FieldWrite::Cell(FieldCell::Graph {
            text: 'b',
            width: 1,
        }),
        super::FieldWrite::Cell(FieldCell::Graph {
            text: 'c',
            width: 1,
        }),
    ]);
    let first = buffer.fill_pass(3).unwrap();
    assert_eq!((first.end, first.width), (1, 1));
    assert_eq!(buffer.cells()[2], FieldCell::BreakableBlank);
    buffer.advance_past(first.end);
    buffer.consume_break_blanks();
    assert_eq!(buffer.resume_offset(), 3);
    let second = buffer.fill_pass(3).unwrap();
    assert_eq!((second.end, second.width), (5, 2));

    let mut tail = FieldBuffer::default();
    tail.push_non_breaking_blank();
    let pass = tail.fill_pass(3).unwrap();
    assert_eq!((pass.end, pass.width), (1, 1));
    assert!(tail.only_ignorable_remainder(false));
    assert!(!tail.only_ignorable_remainder(true));
    assert_eq!(tail.brtrsp_tail_sweep(tail.cells(), 0, 0, true), 24);
}

#[test]
fn native_receipt_owns_normalized_keep_cells_without_mutating_source_writes() {
    let mut buffer = FieldBuffer::default();
    buffer.apply_writes(&[
        super::FieldWrite::Cell(FieldCell::Graph {
            text: 'a',
            width: 1,
        }),
        super::FieldWrite::Cell(FieldCell::BreakableBlank),
        super::FieldWrite::Cell(FieldCell::NonBreakingBlank),
        super::FieldWrite::Cell(FieldCell::Graph {
            text: 'b',
            width: 1,
        }),
        super::FieldWrite::Cell(FieldCell::Graph {
            text: 'c',
            width: 1,
        }),
    ]);
    let receipt = buffer.flush_receipt(
        super::FillTargets {
            first: 3 * 24,
            rest: 3 * 24,
            unbounded: false,
        },
        false,
    );
    let super::FlushReceipt::Accepted { passes, .. } = &receipt else {
        panic!("all native passes accept");
    };
    assert_eq!(
        passes.iter().map(|p| (p.end, p.width)).collect::<Vec<_>>(),
        [(1, 1), (5, 2)]
    );
    assert_eq!(receipt.native_cells()[2], FieldCell::BreakableBlank);
    assert_eq!(buffer.cells()[2], FieldCell::NonBreakingBlank);
}
