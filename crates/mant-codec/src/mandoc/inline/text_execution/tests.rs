use super::*;

#[test]
fn contiguous_deferred_source_spaces_use_one_run_receipt() {
    use crate::mandoc::inline::flow::field_buffer::FieldWrite;

    // The exact .No operand ran pinned term.c::term_flushln()205-207
    // first: the continuation sweeps all 8,192 SP, leaving ALPHA/BETA.
    let operand = format!("ALPHA\\p{}BETA", " ".repeat(8192));
    let mut font = FontState::new();
    let mut zero = ZeroAdvanceState::new();
    zero.set_native_word_owner(1);
    let execution = parse_roff_text_with_zero_advance(
        &operand,
        TextExecutionContext {
            font: &mut font,
            zero_advance: &mut zero,
            defer_leading_blanks: false,
            policy: TextExecutionPolicy {
                recognize_generated_references: false,
                record_native_cells: true,
                field_authoritative: true,
                word_space: crate::mandoc::roff_escape::WordSpace::Breakable,
            },
        },
    );
    assert_eq!(execution.native_writes.len(), 11);
    assert!(matches!(
        execution.native_writes[6],
        FieldWrite::DeferredBlanks { count: 8192, .. }
    ));
    assert_eq!(execution.output.len(), 3);
    assert_eq!(mant_ir::inline_plain_text(&execution.output), "ALPHABETA");
}

#[test]
fn function_word_policy_changes_only_literal_source_blanks() {
    // Fn/Fo exact full sources for literal, escaped, numbered and motion
    // blanks first ran all five pinned profiles. NBRWORD changes only
    // ordinary source SP in term.c:593-608. Unicode escaped spaces keep
    // their encode1 path and \N'32' remains ordinary SP. ESCAPE_HORIZ
    // itself writes NBRSP regardless of NBRWORD (term.c:688-696).
    use crate::mandoc::inline::flow::field_buffer::{FieldCell, FieldWrite};
    use crate::mandoc::roff_escape::WordSpace;
    for (source, text, nonbreaking, intrinsic) in [
        ("alpha beta", "alpha beta", 1, 0),
        (r"alpha\ beta", "alpha\u{a0}beta", 0, 0),
        (r"alpha\N'32'beta", "alpha beta", 0, 0),
        (r"alpha\h'1n'beta", "alpha beta", 0, 1),
    ] {
        for spaces in [WordSpace::Breakable, WordSpace::NonBreaking] {
            let mut font = FontState::new();
            let mut zero = ZeroAdvanceState::new();
            let execution = parse_roff_text_with_zero_advance(
                source,
                TextExecutionContext {
                    font: &mut font,
                    zero_advance: &mut zero,
                    defer_leading_blanks: false,
                    policy: TextExecutionPolicy {
                        recognize_generated_references: false,
                        record_native_cells: true,
                        field_authoritative: true,
                        word_space: spaces,
                    },
                },
            );
            assert_eq!(mant_ir::inline_plain_text(&execution.output), text);
            assert_eq!(
                execution
                    .native_writes
                    .iter()
                    .filter(|write| matches!(write, FieldWrite::Cell(FieldCell::NonBreakingBlank)))
                    .count(),
                intrinsic
                    + if spaces == WordSpace::NonBreaking {
                        nonbreaking
                    } else {
                        0
                    },
                "{source}",
            );
        }
    }
}

#[test]
fn isolated_words_consume_pending_markers_at_zero_width_breakpoints() {
    // Exact man and tbl inputs first ran on the pristine five profiles.
    // term_fill()287-305 stops at ASCII_BREAK exactly as at a blank,
    // adding no width or glyph; nbr==0 rejects the unaccepted suffix.
    for (source, expected) in [
        (r"X\:Y", "XY"),
        (r"X\p\:Y", "X\nY"),
        (r"\p\:Y", ""),
        (r"\zX\p\:Y", "X\nY"),
        (r"\zY\p\: \fBZ", "Y\nZ"),
    ] {
        let output = parse_roff_text_with_state(source, &mut FontState::new(), false);
        assert_eq!(
            mant_ir::inline_plain_text(&output),
            expected,
            "{source}: {output:#?}"
        );
    }
}

#[test]
fn breakpoint_writes_do_not_claim_zero_width_graph_acceptance() {
    // Exact X\:Y source ran first: ASCII_BREAK is an ordered buffer cell
    // and break candidate, never the graph-class ASCII_NBRZW of \&.
    use crate::mandoc::inline::flow::field_buffer::{FieldCell, FieldWrite};
    let mut font = FontState::new();
    let mut zero = ZeroAdvanceState::new();
    let execution = parse_roff_text_with_zero_advance(
        r"X\:Y",
        TextExecutionContext {
            font: &mut font,
            zero_advance: &mut zero,
            defer_leading_blanks: false,
            policy: TextExecutionPolicy {
                recognize_generated_references: false,
                record_native_cells: true,
                field_authoritative: true,
                word_space: crate::mandoc::roff_escape::WordSpace::Breakable,
            },
        },
    );
    assert!(matches!(
        execution.native_writes.as_slice(),
        [
            FieldWrite::Cell(FieldCell::Graph { text: 'X', .. }),
            FieldWrite::Cell(FieldCell::Breakpoint),
            FieldWrite::Cell(FieldCell::Graph { text: 'Y', .. })
        ]
    ));
    assert!(!execution.word_zero_graph);
    assert_eq!(mant_ir::inline_plain_text(&execution.output), "XY");
}

#[test]
fn field_recording_does_not_select_another_text_executor() {
    // The exact recording-mode.roff input passed pristine CVS ASCII,
    // UTF-8 and lint. term_word() buffers X, ESCAPE_BREAK and
    // ASCII_NBRZW in order; recording is independent of presentation.
    let run = |record| {
        let mut font = FontState::new();
        let mut zero = ZeroAdvanceState::new();
        parse_roff_text_with_zero_advance(
            "X\\p\\&",
            TextExecutionContext {
                font: &mut font,
                zero_advance: &mut zero,
                defer_leading_blanks: false,
                policy: TextExecutionPolicy {
                    recognize_generated_references: false,
                    record_native_cells: record,
                    field_authoritative: false,
                    word_space: crate::mandoc::roff_escape::WordSpace::Breakable,
                },
            },
        )
    };
    let ordinary = run(false);
    let field = run(true);
    assert_eq!(ordinary.output, field.output);
    assert_eq!(
        ordinary.pending_word_end_break,
        field.pending_word_end_break
    );
    assert_eq!(ordinary.source_continuation, field.source_continuation);
    assert!(ordinary.native_writes.is_empty());
    assert_eq!(field.native_writes.len(), 3);
}

#[test]
fn unknown_recovery_preserves_source_order_without_consuming_zero_advance() {
    // Exact complete roff fixtures first verified with pristine CVS.
    // term.c:620-638 writes NBRZW directly; the following encode1 glyph
    // alone consumes BACKAFTER/BACKBEFORE. Recovery spelling remains
    // visible under ManT's reading contract and retains its source style.
    for (source, expected) in [
        (r"\z\[unknownname]YZ", "Z"),
        (r"\z\N'256'YZ", "Z"),
        (r"\zX\[unknownname]", r"X\[unknownname]"),
        (r"\zX\[unknownname]Y", r"\[unknownname]Y"),
        (r"\zX\[unknownname]\[u03B1]", "\\[unknownname]α"),
        (r"\zX\[unknownname] Y", r"X\[unknownname]Y"),
        (r"X\[unknownname]Y", r"X\[unknownname]Y"),
    ] {
        let output = parse_roff_text_with_state(source, &mut FontState::new(), false);
        assert_eq!(mant_ir::inline_plain_text(&output), expected, "{source}");
    }
    let output = parse_roff_text_with_state(r"\zX\[unknownname]\fBY", &mut FontState::new(), false);
    assert_eq!(mant_ir::inline_plain_text(&output), r"\[unknownname]Y");
    assert!(matches!(output.last(), Some(Inline::Strong { children })
        if mant_ir::inline_plain_text(children) == "Y"));
    assert!(matches!(output.first(), Some(Inline::Text { value })
        if value == r"\[unknownname]"));
}

#[test]
fn positive_motion_consumes_backbefore_before_buffering_remaining_cells() {
    // Complete mdoc sources first ran the pristine five profiles. The
    // first positive HORIZ cell consumes BACKBEFORE (term.c:692-699),
    // whereas the absolute zero-advance control leaves it armed.
    for (source, expected) in [
        (r"\zX\h'1n'Y", "XY"),
        (r"\zX\h'2n'Y", "X Y"),
        (r"\zX\h'|1n'Y", "Y"),
        (r"\zX \h'1n'Y", "X Y"),
        (r"\zX \h'2n'Y", "X  Y"),
    ] {
        let output = parse_roff_text_with_state(source, &mut FontState::new(), false);
        assert_eq!(mant_ir::inline_plain_text(&output), expected, "{source}");
    }
}
