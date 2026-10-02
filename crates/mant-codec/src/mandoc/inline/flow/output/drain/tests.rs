use super::*;
use crate::mandoc::formatter::FormatterState;
use crate::mandoc::inline::flow::{AuthorBreakEffect, FieldFlags};

const MDOC_HEADER: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

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
