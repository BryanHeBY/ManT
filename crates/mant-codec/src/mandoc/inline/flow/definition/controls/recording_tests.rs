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

#[test]
fn definition_field_session_reenters_after_owner_handoff() {
    let mut builder = InlineBuilder::with_spacing(true);
    builder.inherit_author_execution_with_effect(
        crate::mandoc::formatter::AuthorFlow::default(),
        false,
        AuthorBreakEffect::Field {
            gap_cells: 1,
            body_width_columns: 6,
            field_width_columns: 4,
            flags: FieldFlags::hang(),
        },
    );
    builder.execution.definition = None;
    assert!(!builder.in_definition_field());
    builder.ensure_definition_field_session();
    assert!(builder.in_definition_field());
}

#[test]
fn pre_br_consumes_old_flags_and_receipt_once_before_post_changes() {
    // Each complete source ran pristine ASCII/UTF-8/HTML/tree/lint first.
    // roff_term.c:69-78 / 233-236 uses the same pre-br for br, fi/nf and ti.
    // term_newln sets NOSPACE before testing lastcol/viscol (term.c:475-481).
    let head =
        ".Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for (kind, words, request, column) in [
        ("tag", ".No \\&\n", ".nf", false),
        ("tag", ".No A\\c\n", ".nf", false),
        ("hang", ".No A\n", ".fi", false),
        ("column", ".No A\n", ".ti", true),
        ("hang", ".No X\\p\n.No \"\\p Y\"\n", ".br", false),
    ] {
        let opening = if column {
            ".Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n".to_owned()
        } else {
            format!(".Bl -{kind} -width 8n\n.It Xo\n")
        };
        let closing = if column {
            ".Xc Ta RIGHT\n.El\n"
        } else {
            ".Xc\n.No BodyWord\n.El\n"
        };
        let after = if kind == "tag" && words.contains("A\\c") {
            "MID"
        } else {
            "AFTER"
        };
        let source =
            format!("{head}{opening}{words}{request}\n.No {after}\n{closing}.Sh NEXT\n.No END\n");
        let document = crate::mandoc::parse_plain_manual(
            std::path::Path::new("pre-br-once.1"),
            source.as_bytes(),
        )
        .expect("lower exact pre-br input");
        let trace = super::super::control_trace::take();
        assert!(trace.nospace_before_capture, "{source}: {trace:?}");
        assert!(trace.cells > 0, "{source}: {trace:?}");
        assert_eq!(trace.viscol, 0, "{source}: {trace:?}");
        assert_eq!(trace.captures, 1, "{source}: {trace:?}");
        assert_eq!(trace.projections, 1, "{source}: {trace:?}");
        assert_eq!(trace.retirements, 1, "{source}: {trace:?}");
        assert_eq!(
            trace.phases,
            ["nospace", "capture", "project", "retire", "post-flags"]
        );
        assert!(trace.old_flags.contains(FieldFlag::NoBreak), "{trace:?}");
        assert_eq!(
            trace.old_flags.contains(FieldFlag::Brind),
            !column,
            "{trace:?}"
        );
        let post = trace.post_flags.expect("field state survives pre-br");
        assert_eq!(post.contains(FieldFlag::NoBreak), column, "{trace:?}");
        assert!(!post.contains(FieldFlag::Brind), "{trace:?}");
        assert!(!trace.accepted_ends.is_empty(), "{source}: {trace:?}");
        assert!(document.sections.len() >= 3);
    }
}

#[test]
fn column_control_receipt_accounts_for_literal_tab_printing() {
    // Exact source is field-pre-br-widths-0156 in the pristine five-profile
    // fixture. term_field() advances the default .5i stop before B; pre_br
    // keeps column NOBREAK and consumes this first field once.
    let source = ".Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n.No \"A\tB\"\n.br\n.No AFTER\n.Xc Ta RIGHT\n.El\n.Sh NEXT\n.No END\n";
    crate::mandoc::parse_plain_manual(
        std::path::Path::new("column-tab-receipt.1"),
        source.as_bytes(),
    )
    .unwrap();
    let trace = super::super::control_trace::take();
    assert_eq!(trace.printed_column, Some(6));
    assert_eq!(trace.ends_row, Some(false));
}

#[test]
fn hang_literal_tabs_use_final_print_receipt_for_body_seam() {
    // These six exact inputs are the pristine five-profile F matrix cases
    // core-1159/1160/1191 and widths-0093/0094/0095. HEAD post prints after
    // Xo restores offset (mdoc_term.c:437-439,946-955). Earlier .mc/pre-br
    // receipts cannot permanently consume that final word separator.
    let head =
        ".Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for (width, word, controls, gap) in [
        (8, "\t", ".nf\n.No MID\n.fi\n", 0),
        (32, "\t", ".nf\n.No MID\n.fi\n", 0),
        (4, "\t", ".mc |\n.nf\n.fi\n.mc\n", 1),
        (8, "A\tB", ".br\n", 0),
        (8, "A\tB", ".nf\n.fi\n", 0),
        (8, "A\tB", ".ti\n", 0),
    ] {
        let source = format!(
            "{head}.Bl -hang -width {width}n\n.It Xo\n.No \"{word}\"\n{controls}.No AFTER\n.Xc\n.No BodyWord\n.El\n.Sh NEXT\n.No END\n"
        );
        let document = crate::mandoc::parse_plain_manual(
            std::path::Path::new("tab-seam.1"),
            source.as_bytes(),
        )
        .unwrap();
        let section = document
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == "DESCRIPTION")
            .unwrap();
        let mant_ir::Block::DefinitionList { items, .. } = &section.blocks[0] else {
            panic!("missing actual definition owner: {source}")
        };
        let item = &items[0];
        assert!(mant_ir::inline_plain_text(&item.terms[0]).contains('\t'));
        assert_eq!(item.layout.min_term_gap_columns, gap, "{source}");
        assert_eq!(
            item.head_body_relation.joins_without_separator(),
            gap == 0,
            "{source}"
        );
    }
}
