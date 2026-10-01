use super::*;
use crate::mandoc::formatter::AuthorFlow;
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
        let mut builder = InlineBuilder::new();
        builder.execution.macro_set = parsed.document.macro_set;
        builder.inherit_author_execution_with_effect(
            AuthorFlow::default(),
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
        let (output, _, occupied) = builder.finish_formatter_line(true);
        assert_eq!(InlineBuilder::native_field_device_views(), 1, "{source}");
        assert_eq!(occupied, operand != "\\&", "{source}");
        assert_eq!(
            crate::mandoc::inline::plain_text(&output).contains('X'),
            operand != "\\&",
            "{source}"
        );
    }
}
