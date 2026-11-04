use super::*;
use crate::mandoc::formatter::FormatterState;
use crate::mandoc::inline::flow::{AuthorBreakEffect, FieldFlags};

const MDOC_HEADER: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

#[test]
fn direct_flush_and_newline_have_distinct_word_flags_after_cell_retirement() {
    use crate::mandoc::inline::flow::NativeWordSpacing;

    // Exact Fn/Fo + empty-word + Sm-on sources ran all five pristine
    // profiles first (word_spacing/flush-*). term_flushln() retires cells
    // without altering NOSPACE; term_newln() selects it before flushing.
    for newline in [false, true] {
        let mut builder = InlineBuilder::with_spacing(true);
        builder.append_text("call();");
        assert_eq!(builder.native_word_spacing, NativeWordSpacing::Separate);
        if newline {
            builder.execute_native_newline();
        } else {
            builder.flush_execution_line();
        }
        assert_eq!(
            builder.native_word_spacing,
            if newline {
                NativeWordSpacing::NoSpace
            } else {
                NativeWordSpacing::Separate
            }
        );
        builder.append_text("");
        assert_eq!(
            builder.execution.word_input().0.cells().len(),
            usize::from(!newline)
        );
        builder.set_spacing("on");
        builder.append_text("SECOND");
        // The empty word and the successor each consume the incoming flag.
        assert_eq!(
            builder.execution.word_input().0.cells().len(),
            8 - usize::from(newline)
        );
    }
}

fn text_operand<'n>(node: &'n libmandoc_rs::Node, text: &str) -> Option<&'n libmandoc_rs::Node> {
    if node.decoder_text() == Some(text) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|node| text_operand(node, text))
}

#[test]
fn actual_head_post_uses_one_device_view_for_visible_invisible_and_pending_cells() {
    // The four complete sources ran ASCII/UTF-8/lint/HTML/tree pristine
    // before this counter: visible and pending TAG heads share the BODY
    // row, OHANG NBRZW completes an empty row, and man TP keeps its fitting
    // tag. term_flushln()113-253 supplies one real HEAD post device result;
    // mdoc_term.c409-439 restores node geometry after that post. Inspecting
    // invisible output and returning occupied viscol consume that same result.
    let cases = [
        (
            format!("{MDOC_HEADER}.Bl -tag -width 16n\n.It X\n.No BODY\n.El\n.Sh NEXT\n.No END\n"),
            "X",
            FieldFlags::tag(false),
            false,
        ),
        (
            format!("{MDOC_HEADER}.Bl -ohang\n.It \\&\n.No BODY\n.El\n.Sh NEXT\n.No END\n"),
            "\\&",
            FieldFlags::inset(),
            false,
        ),
        (
            format!("{MDOC_HEADER}.Bl -tag -width 16n\n.It Xo\n.No \\zX\n.Xc\n.No BODY\n.El\n.Sh NEXT\n.No END\n"),
            "\\zX",
            FieldFlags::tag(false),
            true,
        ),
        (
            ".TH TEST 1 \"September 28, 2026\"\n.SH NAME\ntest \\- probe\n.SH DESCRIPTION\n.TP 16\nX\nBODY\n.SH NEXT\nEND\n".to_owned(),
            "X",
            FieldFlags::man_head(true),
            false,
        ),
    ];
    for (source, operand, flags, pending_glyph) in cases {
        let parsed = libmandoc_rs::Parser::default()
            .parse_bytes("head-post-device-view.1", source.as_bytes())
            .unwrap();
        let mut formatter = FormatterState::default();
        formatter.execution.macro_set = parsed.document.macro_set;
        let mut builder = formatter.begin_inline_session(
            true,
            false,
            AuthorBreakEffect::Field {
                gap_cells: u8::try_from(flags.trailspace()).unwrap(),
                body_width_columns: 16,
                field_width_columns: 16,
                flags,
            },
        );
        builder.begin_definition_head_consumption();
        crate::mandoc::inline::append_text_node(
            &mut builder,
            text_operand(&parsed.document.root, operand).unwrap(),
        );
        assert_eq!(builder.zero_advance.has_buffered_glyph(), pending_glyph);
        InlineBuilder::reset_native_field_device_views();
        let finished = formatter.finish_inline_line_with_rows(builder, true);
        assert_eq!(InlineBuilder::native_field_device_views(), 1, "{source}");
        assert_eq!(
            formatter.definition_head_row_occupied(),
            operand != "\\&",
            "{source}"
        );
        assert_eq!(
            crate::mandoc::inline::plain_text(&finished.output).contains('X'),
            operand != "\\&",
            "{source}"
        );
    }
}

#[test]
fn capacity_seam_observation_borrows_one_real_head_post_receipt() {
    // These exact full sources ran the five pristine profiles first.
    // pre_br clears BRIND/NOBREAK/trailspace, followed by the source word;
    // It HEAD post then consumes that buffer exactly once (roff_term.c:
    // 69-78; mdoc_term.c:930-964). Reading equality or overrun from the
    // captured device tail must not perform another term_flushln sweep.
    fn item_head(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::Node> {
        if node.kind == libmandoc_rs::NodeKind::Head && node.macro_token.as_deref() == Some("It") {
            Some(node)
        } else {
            node.children.iter().find_map(item_head)
        }
    }
    let header =
        ".Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for length in [3, 4, 5] {
        let word = "X".repeat(length);
        let source =
            format!("{header}.Bl -hang -width 2n\n.It Xo\n.br\n.No {word}\n.Xc\n.No BODY\n.El\n");
        let parsed = libmandoc_rs::Parser::default()
            .parse_bytes("capacity-head-post.1", source.as_bytes())
            .unwrap();
        let mut formatter = FormatterState::default();
        formatter.execution.macro_set = parsed.document.macro_set;
        let mut builder = formatter.begin_inline_session(
            true,
            false,
            AuthorBreakEffect::Field {
                gap_cells: 1,
                body_width_columns: 4,
                field_width_columns: 4,
                flags: FieldFlags::hang(),
            },
        );
        builder.begin_definition_head_consumption();
        builder.set_definition_native_margin(4 * 24);
        // Execute the real Xo subtree, including its geometry restoration
        // before It HEAD post (print_mdoc_node():437-439). A direct text
        // append after pre_br would leave the child-only BODY offset live.
        crate::mandoc::inline::append_inline_nodes(
            &mut builder,
            &item_head(&parsed.document.root).unwrap().children,
            None,
        );
        InlineBuilder::reset_native_field_device_views();
        let finished = formatter.finish_inline_line_with_rows(builder, true);
        assert_eq!(InlineBuilder::native_field_device_views(), 1, "{source}");
        assert_eq!(formatter.execution.concat_flush_source, length >= 4);
        assert!(formatter.definition_head_row_occupied(), "{source}");
        assert_eq!(crate::mandoc::inline::plain_text(&finished.output), word);
    }
}

#[test]
fn a_real_newline_selects_nospace_even_after_a_graphless_word() {
    // Exact filled/no-fill START/sp-1/No""/br/No\&/BODY sources ran
    // pristine before these checks. term_newln()475-481 updates NOSPACE
    // before its conditional flush; term_flushln()233-253 never clears it.
    // The first post-break NBRZW writes no separator, while BODY writes
    // precisely its own one automatic blank (term_word()573-589).
    let source = format!("{MDOC_HEADER}.No START\n.sp -1\n.No \"\"\n.br\n.No \\&\n.No BODY\n");
    let parsed = libmandoc_rs::Parser::default()
        .parse_bytes("graphless-word-newline.1", source.as_bytes())
        .unwrap();
    for starts_occupied in [false, true] {
        let mut builder = InlineBuilder::new();
        builder.tighten_next_boundary();
        if starts_occupied {
            builder.append_text("START");
        } else {
            crate::mandoc::inline::append_text_node(
                &mut builder,
                text_operand(&parsed.document.root, "").unwrap(),
            );
        }
        assert_eq!(builder.execution.boundary, PendingBoundary::Ordinary);
        builder.hard_break();
        assert_eq!(builder.execution.boundary, PendingBoundary::Tight);
        crate::mandoc::inline::append_text_node(
            &mut builder,
            text_operand(&parsed.document.root, "\\&").unwrap(),
        );
        assert_eq!(builder.execution.pending_breakable_spaces, 0);
        builder.append_text("BODY");
        let text = crate::mandoc::inline::plain_text(&builder.finish_preserving_rows());
        assert_eq!(
            text,
            if starts_occupied {
                "START\n BODY"
            } else {
                " BODY"
            }
        );
    }
}

#[test]
fn an_ir_owner_drain_keeps_the_executed_empty_words_separator_register() {
    // The direct counterparts ran all five pristine profiles and are frozen
    // in word_spacing. An owner drain inserts no native word/newline between
    // them: only the IR destination changes. Empty words update NOSPACE
    // without writing a cell (term_word():573-589), so Sm-on's p->col check
    // must leave opposite results after an on-word and an off-word.
    for (initial, setting, expected) in [
        ("off", "on", "SECOND"),
        ("off", "", "SECOND"),
        ("on", "on", " SECOND"),
        ("on", "off", " SECOND"),
    ] {
        let mut first_owner = InlineBuilder::new();
        first_owner.set_spacing(initial);
        first_owner.execute_empty_word();
        let (output, _, execution) = first_owner.finish_preserving_execution();
        assert_eq!(crate::mandoc::inline::plain_text(&output), "");
        assert!(execution.plain_input.buffer.is_empty());
        let mut next_owner = InlineBuilder::from_parts(Vec::new(), execution);
        next_owner.set_spacing(setting);
        next_owner.append_text("SECOND");
        assert_eq!(
            crate::mandoc::inline::plain_text(&next_owner.finish()),
            expected,
            "{initial}/{setting}"
        );
    }
}

#[test]
fn geometry_without_an_author_uses_the_same_word_ledger_as_plain_recording() {
    // The exact recovered `No arg / Nm prog tail last` and independent
    // fragment ran all pristine profiles before this check. Nm can leave
    // geometry in an authorless synthetic session (termp_nm_pre/post), but
    // every following term_word still uses one input buffer. In particular
    // reading NOSPACE from an unused geometry buffer joins tail/last.
    for authored in [false, true] {
        let mut builder = InlineBuilder::new();
        if authored {
            builder
                .inherit_author_execution(crate::mandoc::formatter::AuthorFlow::Automatic, false);
        }
        builder.definition_state_mut();
        builder.append_text("prog");
        builder.append_text("tail");
        builder.append_text("last");
        assert_eq!(
            crate::mandoc::inline::plain_text(&builder.finish()),
            "prog tail last",
            "authored={authored}"
        );

        let mut builder = InlineBuilder::new();
        if authored {
            builder
                .inherit_author_execution(crate::mandoc::formatter::AuthorFlow::Automatic, false);
        }
        builder.definition_state_mut();
        builder.set_spacing("off");
        builder.execute_empty_word();
        builder.set_spacing("on");
        builder.append_text("SECOND");
        assert_eq!(
            crate::mandoc::inline::plain_text(&builder.finish()),
            "SECOND"
        );
    }
}
