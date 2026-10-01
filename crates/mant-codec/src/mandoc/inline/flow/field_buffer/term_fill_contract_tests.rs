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
