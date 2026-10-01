//! Section facades and literal leaves preserve the same source row stream.
use mant_loader::{load_markdown_text, load_roff_bytes};
use mant_render::{render_excerpt_text, render_query_text, render_query_text_with};

#[cfg(feature = "roff")]
#[path = "text_source_flow/no_fill_rows.rs"]
mod no_fill_rows;

#[test]
fn native_section_pd_and_explicit_requests_compose_once() {
    // mandoc CVS pre_SH consumes pardist; preceding .sp remains independent.
    for pd in [0, 1, 2] {
        for request in [0, 3] {
            for heading in ["SH", "SS"] {
                let source = format!(
                    ".TH PROBE 1\n.PD {pd}\n.SH FIRST\nALPHA\n.sp {request}\n.{heading} SECOND\nBETA\n"
                );
                let content = load_roff_bytes(source.as_bytes()).unwrap();
                let text = render_query_text(&content);
                let heading_indent = if heading == "SS" { "  " } else { "" };
                assert!(
                    text.contains(&format!(
                        "ALPHA{}{heading_indent}SECOND\n",
                        "\n".repeat(pd + request + 1)
                    )),
                    "{source}\n{text:?}"
                );
                assert_eq!(
                    render_query_text_with(&content, |_, value| value.into()),
                    text
                );
            }
        }
    }
}

#[test]
fn no_fill_source_line_precedes_state_only_requests_inside_man_links() {
    // Exact UR/MT x ft/PD/ta/ll/po x continuation inputs were run with the
    // fixed CVS -Tascii/-Tutf8/-Thtml/-Tlint oracle. man_term.c::print_man_node() handles
    // NODE_NOFILL | NODE_LINE before the roff request handler; a preceding
    // \c suppresses only that source-line break. post_UR() (also MT's post)
    // executes literal ASCII '<' and '>' on both terminal devices; the mdoc
    // enclosure catalog glyphs do not apply to these generated man words.
    for (open, close, target) in [
        ("UR", "UE", "https://example.org"),
        ("MT", "ME", "user@example.org"),
    ] {
        for request in ["ft B", "PD 0", "ta 8n", "ll 40n", "po 1n"] {
            for continued in [false, true] {
                let join = if continued { "\\c" } else { "" };
                let input = format!(
                    ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n.nf\n.{open} {target}\nlabel{join}\n.{request}\n.{close}\nafter\n"
                );
                let query = load_roff_bytes(input.as_bytes()).unwrap();
                let text = render_query_text(&query);
                let boundary = if continued {
                    format!("label<{target}>\nafter")
                } else {
                    format!("label\n<{target}>\nafter")
                };
                assert_eq!(
                    text.split_once("DESCRIPTION\n").unwrap().1,
                    boundary,
                    "{input}\n{text:?}"
                );
            }
        }
    }
}

#[test]
fn native_line_requests_release_a_continued_literal_row() {
    // Fixed CVS roff_term.c::roff_term_pre_ti()/pre_br() executes term_newln()
    // before applying indentation. in/ce/rj use the same physical boundary.
    for request in ["ti 8n", "in 1n", "ce 1", "rj 1"] {
        let input = format!(
            ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n.nf\nALPHA\\c\n.{request}\nBETA\n.fi\n"
        );
        let text = render_query_text(&load_roff_bytes(input.as_bytes()).unwrap());
        let tail = text.split("ALPHA").nth(1).unwrap_or_default();
        assert!(tail.starts_with('\n'), "{input}\n{text:?}");
        assert!(tail.contains("BETA"), "{input}\n{text:?}");
    }
}

#[test]
fn visited_empty_text_consumes_negative_space_without_executing_a_word() {
    // Every exact case was checked with fixed CVS -Tascii/-Tlint. The empty
    // TEXT branch in man_term.c and mdoc_term.c calls term_vspace(), while
    // man_term.c::pre_alternate() calls term_word() for BR operands and \& is
    // an actual zero-width formatter word that clears skipvsp.
    for (name, body, expected) in [
        ("one-blank", "\n", "BEFORE\nAFTER"),
        ("two-blanks", "\n\n", "BEFORE\n\nAFTER"),
        ("bold-empty", ".B \"\"\n", "BEFORE\nAFTER"),
        ("italic-empty", ".I \"\"\n", "BEFORE\nAFTER"),
        ("alternate-empty", ".BR \"\" \"\"\n", "BEFORE\n\nAFTER"),
        ("zero-width", "\\&\n", "BEFORE\n\n\nAFTER"),
    ] {
        let input = format!(
            ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n.nf\nBEFORE\n.sp -2\n{body}.PP\nAFTER\n"
        );
        let text = render_query_text(&load_roff_bytes(input.as_bytes()).unwrap());
        assert!(text.contains(expected), "{name}: {input}\n{text:?}");
    }
    for (name, body, expected) in [
        ("one-blank", "\n", "BEFORE\nAFTER"),
        ("two-blanks", "\n\n", "BEFORE\n\nAFTER"),
        ("empty-emphasis", ".Em \"\"\n", "BEFORE\n\nAFTER"),
        ("zero-width", "\\&\n", "BEFORE\n\n\nAFTER"),
    ] {
        let input = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\nBEFORE\n.sp -2\n{body}.Pp\nAFTER\n.fi\n"
        );
        let text = render_query_text(&load_roff_bytes(input.as_bytes()).unwrap());
        assert!(text.contains(expected), "{name}: {input}\n{text:?}");
    }

    // The original one-row review case has no following paragraph request.
    // man_term.c visits the blank TEXT via term_vspace(), cancelling .sp -1.
    let original = ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n.nf\nBEFORE\n.sp -1\n\nAFTER\n";
    let text = render_query_text(&load_roff_bytes(original.as_bytes()).unwrap());
    assert!(text.contains("BEFORE\nAFTER"), "{text:?}");
    let continued = ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n.nf\nBEFORE\\c\n.sp -1\n\nAFTER\n";
    let text = render_query_text(&load_roff_bytes(continued.as_bytes()).unwrap());
    assert!(text.contains("BEFORE\nAFTER"), "{text:?}");
    // term_newln() keeps TERMP_NONEWLINE, even when several visited empty
    // TEXT nodes intervene. The result is one physical break, not blank rows.
    let continued_blanks =
        ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n.nf\nBEFORE\\c\n\n\nAFTER\n.fi\n";
    let text = render_query_text(&load_roff_bytes(continued_blanks.as_bytes()).unwrap());
    assert!(text.contains("BEFORE\nAFTER"), "{text:?}");
    for macro_name in ["B", "I", "R"] {
        let input = format!(
            ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\nBEFORE\\c\n.{macro_name} \"\"\nAFTER\n"
        );
        let text = render_query_text(&load_roff_bytes(input.as_bytes()).unwrap());
        assert!(text.contains("BEFORE\nAFTER"), "{input}\n{text:?}");
    }

    // The same native empty-TEXT rule applies in filled flow. The BR operands
    // differ because pre_alternate() calls term_word() directly for each one.
    for (name, body, expected) in [
        ("raw", "\n", "BEFORE\nAFTER"),
        ("bold", ".B \"\"\n", "BEFORE\nAFTER"),
        ("italic", ".I \"\"\n", "BEFORE\nAFTER"),
        ("alternate", ".BR \"\" \"\"\n", "BEFORE\n\nAFTER"),
        ("zero-width", "\\&\n", "BEFORE\n\n\nAFTER"),
    ] {
        let input = format!(
            ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\nBEFORE\n.sp -2\n{body}.PP\nAFTER\n"
        );
        let text = render_query_text(&load_roff_bytes(input.as_bytes()).unwrap());
        assert!(text.contains(expected), "{name}: {input}\n{text:?}");
    }
    // CVS term_word(" ") from B's combined operands clears skipvsp, then
    // term_flushln() ends its whitespace-only cell before PP adds distance.
    let spaced_word =
        ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\nBEFORE\n.sp -1\n.B \"\" \"\"\n.PP\nAFTER\n";
    let text = render_query_text(&load_roff_bytes(spaced_word.as_bytes()).unwrap());
    assert!(text.contains("BEFORE\n\n\nAFTER"), "{text:?}");
}

#[test]
fn native_newlines_and_ir_drains_keep_source_continuation_until_a_word() {
    // CVS term.c::term_newln()/term_flushln() leave TERMP_NONEWLINE intact;
    // only term_word() consumes it. roff_term_pre_sp() adds debt before its
    // final term_newln(). All exact inputs were run with fixed -Tascii/-Tlint.
    for no_fill in [false, true] {
        for request in ["sp -1", "br", "ti 8n", "in 1n", "nf"] {
            let input = format!(
                ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n{}BEFORE\\c\n.{request}\n.B \"\"\n.PP\nAFTER\n{}",
                if no_fill { ".nf\n" } else { "" },
                if no_fill { ".fi\n" } else { "" }
            );
            let text = render_query_text(&load_roff_bytes(input.as_bytes()).unwrap());
            let expected = if request == "sp -1" {
                "BEFORE\nAFTER"
            } else {
                "BEFORE\n\nAFTER"
            };
            assert!(text.contains(expected), "{input}\n{text:?}");
        }
    }
}

#[test]
fn section_tail_requests_survive_document_and_excerpt_facades() {
    // All exact sources ran pristine ASCII/UTF-8/HTML/tree/lint first.
    // term.c::term_vspace() closes ALPHA, then emits each completed empty
    // row. Each EOF row needs a delimiter independently of ALPHA's close;
    // the terminal footer's separate blank is outside the document body.
    for rows in [1, 2, 3] {
        let source = format!(".TH PROBE 1\n.SH FIRST\nALPHA\n.sp {rows}\n");
        let content = load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&content, false).unwrap();
        let bundle: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let content = bundle.into();
        let tail = format!("ALPHA{}", "\n".repeat(rows + 1));
        assert!(render_query_text(&content).ends_with(&tail));
        assert!(mant_render::render_query_man(&content).ends_with(&tail));
        let excerpt =
            mant_query::select_excerpt(&content, &[mant_protocol::ContentSelector::path("1")])
                .unwrap();
        let json = serde_json::to_string(&excerpt).unwrap();
        let excerpt: mant_protocol::QueryExcerpt = serde_json::from_str(&json).unwrap();
        assert!(render_excerpt_text(&excerpt).ends_with(&tail));
    }
}

#[test]
fn fenced_literal_edges_survive_document_and_excerpt_facades() {
    let content = load_markdown_text(
        "# PROBE\n\n## TEST\n\nBEFORE\n\n```text\n\nALPHA\n\n\n```\n\nAFTER\n",
        None,
    )
    .unwrap();
    let expected = "BEFORE\n\n\nALPHA\n\n\n\nAFTER";
    let text = render_query_text(&content);
    assert!(text.contains(expected), "{text:?}");
    let excerpt =
        mant_query::select_excerpt(&content, &[mant_protocol::ContentSelector::path("1")]).unwrap();
    assert!(render_excerpt_text(&excerpt).contains(expected));
    assert_eq!(
        render_query_text_with(&content, |_, value| value.into()),
        text
    );
}

#[test]
fn definition_explain_preserves_nested_fence_blank_rows() {
    let content = load_markdown_text("# PROBE\n\n## TEST\n\n<!-- mant:entries role=option case=sensitive -->\n- `--example`: BEFORE\n\n  ```text\n\n  ALPHA\n\n\n  ```\n\n  AFTER\n", None).unwrap();
    let explanation = mant_query::explain_query(
        &content,
        &mant_protocol::ExplanationQuery {
            entry: "--example".into(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )
    .unwrap();
    let text = mant_render::render_explanation_text(&explanation);
    assert!(
        text.contains("BEFORE\n\n\n  ALPHA\n\n\n\n  AFTER"),
        "{text:?}"
    );
    assert_eq!(
        mant_render::render_explanation_text_with(&explanation, |_, text| text.into()),
        text
    );
}

#[test]
fn empty_sections_do_not_insert_a_facade_default_gap() {
    let content =
        load_roff_bytes(b".TH PROBE 1\n.PD 2\n.SH FIRST\nALPHA\n.SH EMPTY\n.SH LAST\nBETA\n")
            .unwrap();
    let text = render_query_text(&content);
    // pre_SH deliberately suppresses paragraph distance after an empty SH.
    assert!(text.contains("ALPHA\n\n\nEMPTY\nLAST\nBETA"), "{text:?}");
}
