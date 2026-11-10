//! Detached definition bodies retain the native pending-head line boundary.
use mant_ir::{Block, DefinitionItem};
use mant_loader::load_roff_bytes;
use mant_render::render_query_text;

fn item(query: &mant_ir::ResolvedContent) -> &DefinitionItem {
    let Block::DefinitionList { items, .. } =
        &query.document.as_ref().unwrap().sections[0].blocks[0]
    else {
        panic!("expected definition list")
    };
    items.last().unwrap()
}

#[test]
fn explicit_initial_body_requests_end_short_heads_without_adding_blank_rows() {
    // Pinned CVS roff_term.c maps fi/nf to br; groff agrees on these original
    // inputs. The tag is already buffered when each body request executes.
    for head in [
        ".TP\n.B window\n",
        ".IP window\n",
        ".TP\n.B first\n.TQ\n.B window\n",
    ] {
        for request in [
            ".br\n",
            ".br\n.br\n",
            ".fi\n",
            ".nf\n",
            ".fi\n.nf\n",
            ".sp 0\n",
            ".in +2n\n",
            ".ti 0\n",
            ".ce 0\n",
            ".ce 1\n",
            ".rj 0\n",
            ".rj 1\n",
            ".EX\n",
            ".EE\n",
        ] {
            for prefix in ["", ".ft B\n", ".PD 0\n", "\\fB\n"] {
                let source = format!(".TH PROBE 1\n.SH TEST\n{head}{prefix}{request}BODY\n");
                let query = load_roff_bytes(source.as_bytes()).unwrap();
                let text = render_query_text(&query);
                let owner = item(&query);
                assert!(!owner.inline_term(), "{source}\n{text}");
                let lines: Vec<_> = text.lines().collect();
                let head_row = lines
                    .iter()
                    .position(|line| line.trim() == "window")
                    .unwrap();
                assert_eq!(lines[head_row + 1].trim(), "BODY", "{source}\n{text}");
                if request == ".ti 0\n" {
                    // All twelve exact head/prefix variants reran pristine
                    // before this assertion. roff_term_pre_ti sets offset=0
                    // after pre_br; absolute page zero has no reading pad.
                    assert_eq!(lines[head_row + 1], "BODY", "{source}\n{text}");
                } else {
                    assert!(lines[head_row + 1].starts_with(' '), "{source}\n{text}");
                }
                assert_eq!(
                    mant_ir::validate_document(query.document.as_ref().unwrap()).len(),
                    0
                );
                if prefix == ".ft B\n" {
                    assert!(
                        serde_json::to_string(&owner.description)
                            .unwrap()
                            .contains("strong")
                    );
                }
            }
        }
    }
}

#[test]
fn invisible_inline_controls_do_not_hide_an_initial_body_break() {
    let source = b".TH PROBE 1\n.SH TEST\n.TP\n.B x\n\\&\n.br\nBODY\n";
    let query = load_roff_bytes(source).unwrap();
    let text = render_query_text(&query);
    let owner = item(&query);
    assert!(!owner.inline_term(), "{text}");
    let lines = text.lines().collect::<Vec<_>>();
    let head_row = lines.iter().position(|line| line.trim() == "x").unwrap();
    assert_eq!(lines[head_row + 1].trim(), "BODY", "{text}");
}

#[test]
fn pending_tag_row_consumes_body_breaks_before_explicit_vertical_space() {
    for controls in ["\\p", "\\&\\p"] {
        for (request, blank_rows) in [(".br", 0), (".sp 0", 0), (".sp 1", 1)] {
            let source = format!(
                ".TH PROBE 1\n.SH TEST\n.TP\n.B x\n{controls}\n{request}\nBODY\n.TP\n.B y\nNEXT\n"
            );
            let query = load_roff_bytes(source.as_bytes()).unwrap();
            let text = render_query_text(&query);
            let lines = text.lines().collect::<Vec<_>>();
            let head_row = lines.iter().position(|line| line.trim() == "x").unwrap();
            assert_eq!(
                lines[head_row],
                lines[head_row].trim_end(),
                "{source}\n{text}"
            );
            for line in &lines[head_row + 1..head_row + 1 + blank_rows] {
                assert!(line.trim().is_empty(), "{source}\n{text}");
            }
            assert_eq!(
                lines[head_row + 1 + blank_rows].trim(),
                "BODY",
                "{source}\n{text}"
            );
            assert!(
                lines.iter().any(|line| {
                    let line = line.trim();
                    line.starts_with('y') && line.ends_with("NEXT")
                }),
                "{source}\n{text}"
            );
        }
    }
}

#[test]
fn pending_tag_row_distinguishes_invisible_and_printable_word_end_breaks() {
    for (body_prefix, head_contains, next_line) in [
        ("\\&\\p", "x", Some("BODY")),
        ("A\\p", "A", Some("BODY")),
        ("\\zX\\p", "XBODY", None),
    ] {
        let source =
            format!(".TH PROBE 1\n.SH TEST\n.TP\n.B x\n{body_prefix}\nBODY\n.TP\n.B y\nNEXT\n");
        let query = load_roff_bytes(source.as_bytes()).unwrap();
        let text = render_query_text(&query);
        let lines = text.lines().collect::<Vec<_>>();
        let head_row = lines
            .iter()
            .position(|line| line.trim_start().starts_with('x'))
            .unwrap();
        assert!(lines[head_row].contains(head_contains), "{source}\n{text}");
        assert_eq!(
            lines[head_row],
            lines[head_row].trim_end(),
            "{source}\n{text}"
        );
        if let Some(next_line) = next_line {
            assert_eq!(lines[head_row + 1].trim(), next_line, "{source}\n{text}");
        }
        assert!(
            lines.iter().any(|line| {
                let line = line.trim();
                line.starts_with('y') && line.ends_with("NEXT")
            }),
            "{source}\n{text}"
        );
    }
}

#[test]
fn pending_break_needs_a_live_native_graph_to_continue_after_a_detached_tag() {
    // These exact TP/IP/TQ pages ran all five fixed CVS profiles. Their
    // HEAD posts already printed x, leaving the device row open (man_term.c
    // post_TP/post_IP, term.c:233-253). NBRZW participates in graph
    // acceptance without printing; a bare marker or breakpoint does not
    // (term.c:287-366). Only the accepting pass reaches the BODY glyph.
    const HEADER: &str = ".TH PROBE 1 \"November 10, 2026\"\n.SH NAME\nprobe \\- test pending HEAD\n.SH DESCRIPTION\n";
    for (head, previous_term) in [
        (".TP\n.B x\n", false),
        (".IP x 7\n", false),
        (".TP\n.B z\n.TQ\n.B x\n", true),
    ] {
        for (prefix, accepts_body) in [
            ("\\&\\p", true),
            ("\\p\\&", true),
            ("\\p", false),
            ("\\:\\p", false),
        ] {
            let source = format!("{HEADER}{head}{prefix}\nBODY\n.TP\n.B y\nNEXT\n");
            let rendered = render_query_text(&load_roff_bytes(source.as_bytes()).unwrap());
            let rows: Vec<_> = rendered
                .lines()
                .skip_while(|row| *row != "DESCRIPTION")
                .skip(1)
                .map(|row| row.trim_start_matches(' '))
                .collect();
            let mut expected = Vec::new();
            if previous_term {
                expected.push("z");
            }
            expected.push("x");
            if accepts_body {
                expected.push("BODY");
            }
            expected.extend(["", "y      NEXT"]);
            assert_eq!(rows, expected, "{source}\n{rendered}");
        }
    }
}

#[test]
fn buffered_zero_advance_glyph_precedes_definition_line_requests() {
    // Each exact input was checked with fixed CVS -Tutf8. man_term.c::pre_TP
    // leaves the tag row active; term.c::term_word buffers X before
    // roff_term.c::roff_term_pre_br/sp closes that same row.
    for head in ["key", ".B x"] {
        for (request, blank_rows) in [(".br", 0), (".sp 0", 0), (".sp 1", 1)] {
            let source = format!(".TH PROBE 1\n.SH TEST\n.TP\n{head}\n\\zX\\p\n{request}\nBODY\n");
            let rendered = render_query_text(&load_roff_bytes(source.as_bytes()).unwrap());
            let lines = rendered.lines().map(str::trim).collect::<Vec<_>>();
            let tag = if head == "key" { "key" } else { "x" };
            let tag_row = lines
                .iter()
                .position(|line| line.starts_with(tag))
                .expect("tag row");
            assert!(lines[tag_row].contains('X'), "{source}\n{rendered}");
            let body_row = lines
                .iter()
                .position(|line| *line == "BODY")
                .expect("BODY row");
            assert_eq!(body_row - tag_row - 1, blank_rows, "{source}\n{rendered}");
        }
    }
}

#[test]
fn empty_zero_advance_request_ends_with_its_native_tag_row() {
    // Exact inputs checked with fixed CVS -Tutf8. term_flushln() clears
    // TERMP_BACKAFTER/BACKBEFORE when the pending tag row ends. In pinned
    // CVS, roff_term_pre_ce() calls pre_br(), and man_term.c::pre_in() also
    // calls term_newln() before executing its own effect.
    for request in [
        ".br", ".sp 0", ".nf", ".fi", ".ti 0", ".ce 1", ".rj 1", ".in +2n",
    ] {
        for zero in ["\\z", "\\z "] {
            let source = format!(".TH PROBE 1\n.SH TEST\n.TP\nkey\n{zero}\n{request}\nBODY\n.fi\n");
            let rendered = render_query_text(&load_roff_bytes(source.as_bytes()).unwrap());
            let lines = rendered.lines().map(str::trim).collect::<Vec<_>>();
            let tag_row = lines
                .iter()
                .position(|line| *line == "key")
                .expect("tag row");
            assert_eq!(
                lines.get(tag_row + 1),
                Some(&"BODY"),
                "{source}\n{rendered}"
            );
        }
    }
    let source = b".Dd September 28, 2026\n.Dt PROBE 1\n.Os\n.Sh AUTHORS\n.Bl -tag -width key\n.It key\n\\z\n.An -split\n.An Ada\n.El\n";
    let rendered = render_query_text(&load_roff_bytes(source).unwrap());
    let lines = rendered.lines().map(str::trim).collect::<Vec<_>>();
    let tag_row = lines
        .iter()
        .position(|line| *line == "key")
        .expect("tag row");
    assert_eq!(lines.get(tag_row + 1), Some(&"Ada"), "{rendered}");
}

#[test]
fn pending_head_effects_recurse_through_state_only_wrappers() {
    for wrapper in [
        "B", "I", "SB", "SM", "R", "BI", "BR", "IB", "IR", "RB", "RI",
    ] {
        for (request, blank_rows) in [(".sp 0", 0), (".sp 1", 1)] {
            let source =
                format!(".TH PROBE 1\n.SH TEST\n.TP\n.B x\n.{wrapper} \\&\n{request}\nBODY\n");
            let query = load_roff_bytes(source.as_bytes()).unwrap();
            let text = render_query_text(&query);
            let lines = text.lines().collect::<Vec<_>>();
            let head = lines.iter().position(|line| line.trim() == "x").unwrap();
            let body = lines.iter().position(|line| line.trim() == "BODY").unwrap();
            assert_eq!(body - head - 1, blank_rows, "{source}\n{text}");
        }
    }

    for wrapper in ["Ad", "Ms", "Sx", "Tn", "Mt"] {
        for (request, blank_rows) in [(".sp 0", 0), (".sp 1", 1)] {
            let source = format!(
                ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bl -tag -width Ds\n.It x\n.{wrapper} \\&\n{request}\n.No BODY\n.El\n"
            );
            let query = load_roff_bytes(source.as_bytes()).unwrap();
            let text = render_query_text(&query);
            let lines = text.lines().collect::<Vec<_>>();
            let head = lines.iter().position(|line| line.trim() == "x").unwrap();
            let body = lines.iter().position(|line| line.trim() == "BODY").unwrap();
            assert_eq!(body - head - 1, blank_rows, "{source}\n{text}");
        }
    }

    for (begin, end) in [
        (".Xo", ".Xc"),
        (".Bf -emphasis", ".Ef"),
        (".Bk -words", ".Ek"),
    ] {
        for (request, blank_rows) in [(".sp 0", 0), (".sp 1", 1)] {
            let source = format!(
                ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bl -tag -width Ds\n.It x\n{begin}\n.No \\&\n{end}\n{request}\n.No BODY\n.El\n"
            );
            let query = load_roff_bytes(source.as_bytes()).unwrap();
            let text = render_query_text(&query);
            let lines = text.lines().collect::<Vec<_>>();
            let head = lines.iter().position(|line| line.trim() == "x").unwrap();
            let body = lines.iter().position(|line| line.trim() == "BODY").unwrap();
            assert_eq!(body - head - 1, blank_rows, "{source}\n{text}");
        }
    }

    // Op generates visible delimiters and therefore ends the transparent
    // pending prefix even when its authored operand is zero-width.
    let query = load_roff_bytes(b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bl -tag -width Ds\n.It x\n.Op \\&\n.sp 0\n.No BODY\n.El\n").unwrap();
    assert!(item(&query).inline_term());
    let text = render_query_text(&query);
    assert!(
        text.lines()
            .any(|line| line.contains('x') && line.contains("[]")),
        "{text}"
    );
}

#[test]
fn pending_head_wrapper_effects_follow_mdoc_section_and_sequence_context() {
    for wrapper in ["Cd", "An", "Pf"] {
        for (request, blank_rows) in [(".sp 0", 0), (".sp 1", 1)] {
            let source = format!(
                ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bl -tag -width Ds\n.It x\n.{wrapper} \\&\n{request}\n.No BODY\n.El\n"
            );
            let query = load_roff_bytes(source.as_bytes()).unwrap();
            let text = render_query_text(&query);
            let lines = text.lines().collect::<Vec<_>>();
            let head = lines.iter().position(|line| line.trim() == "x").unwrap();
            let body = lines.iter().position(|line| line.trim() == "BODY").unwrap();
            assert_eq!(body - head - 1, blank_rows, "{source}\n{text}");
        }
    }

    // These rows are the direct result of pinned CVS
    // mdoc_term.c::synopsis_pre(): the real non-transparent sibling token
    // selects term_newln() or term_vspace(), and each call accumulates.
    for (label, wrappers, synopsis_rows) in [
        ("two-ft", ".Ft \\&\n.Ft \\&\n", 2),
        ("three-ft", ".Ft \\&\n.Ft \\&\n.Ft \\&\n", 4),
        ("two-vt", ".Vt \\&\n.Vt \\&\n", 1),
        ("three-vt", ".Vt \\&\n.Vt \\&\n.Vt \\&\n", 2),
        ("vt-ft", ".Vt \\&\n.Ft \\&\n", 2),
        ("in-ft", ".In \\&\n.Ft \\&\n", 2),
        ("cd-vt", ".Cd \\&\n.Vt \\&\n", 1),
        ("cd-cd", ".Cd \\&\n.Cd \\&\n", 1),
    ] {
        for explicit_space in 0..=2 {
            let source = format!(
                ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh SYNOPSIS\n.Bl -tag -width Ds\n.It x\n{wrappers}.sp {explicit_space}\n.No BODY\n.El\n"
            );
            let query = load_roff_bytes(source.as_bytes()).unwrap();
            let text = render_query_text(&query);
            let lines = text.lines().collect::<Vec<_>>();
            let body = lines.iter().position(|line| line.trim() == "BODY").unwrap();
            assert_eq!(
                lines[..body]
                    .iter()
                    .rev()
                    .take_while(|line| line.trim().is_empty())
                    .count(),
                synopsis_rows + explicit_space,
                "{label}: {source}\n{text}"
            );
        }
    }

    // roff_node_prev() is sibling-local: a child Ft cannot become the
    // predecessor of an outer Ft.  The containing wrapper itself yields the
    // one CVS newline here.
    for (begin, end) in [
        (".Bf -emphasis", ".Ef"),
        (".Bk -words", ".Ek"),
        (".Xo", ".Xc"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh SYNOPSIS\n.Bl -tag -width Ds\n.It x\n{begin}\n.Ft \\&\n{end}\n.Ft \\&\n.sp 0\n.No BODY\n.El\n"
        );
        let query = load_roff_bytes(source.as_bytes()).unwrap();
        let text = render_query_text(&query);
        let lines = text.lines().collect::<Vec<_>>();
        let head = lines.iter().position(|line| line.trim() == "x").unwrap();
        let body = lines.iter().position(|line| line.trim() == "BODY").unwrap();
        assert_eq!(body - head - 1, 1, "{source}\n{text}");
    }

    // TERMP_SPLIT and TERMP_NOSPLIT persist across ordinary wrappers.  These
    // exact row counts were measured with the pinned CVS renderer first.
    for (label, authors, rows) in [
        ("automatic", ".An \\&\n.Pf \\&\n.An \\&\n", 1),
        ("nosplit", ".An -nosplit\n.An \\&\n.Pf \\&\n.An \\&\n", 0),
        (
            "split",
            ".An -split\n.Pf \\&\n.An \\&\n.Pf \\&\n.An \\&\n",
            2,
        ),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh AUTHORS\n.Bl -tag -width Ds\n.It x\n{authors}.sp 0\n.No BODY\n.El\n"
        );
        let query = load_roff_bytes(source.as_bytes()).unwrap();
        let text = render_query_text(&query);
        let lines = text.lines().collect::<Vec<_>>();
        let head = lines.iter().position(|line| line.trim() == "x").unwrap();
        let body = lines.iter().position(|line| line.trim() == "BODY").unwrap();
        assert_eq!(body - head - 1, rows, "{label}: {source}\n{text}");
    }
}

#[test]
fn pending_head_boundaries_account_for_the_visible_successor() {
    // These exact gaps were measured with the fixed CVS renderer first.
    for (section, prefix, visible, blank_rows) in [
        ("SYNOPSIS", ".Vt \\&\n", ".In stdio.h\n", 1),
        ("SYNOPSIS", ".Cd \\&\n", ".In stdio.h\n", 0),
        ("AUTHORS", ".An \\&\n", ".An second\n", 0),
    ] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh {section}\n.Bl -tag -width Ds\n.It x\n{prefix}{visible}.No BODY\n.El\n"
        );
        let query = load_roff_bytes(source.as_bytes()).unwrap();
        let text = render_query_text(&query);
        let lines = text.lines().collect::<Vec<_>>();
        let head = lines.iter().position(|line| line.trim() == "x").unwrap();
        let visible = lines
            .iter()
            .position(|line| line.contains("stdio.h") || line.contains("second"))
            .unwrap();
        assert_eq!(visible - head - 1, blank_rows, "{source}\n{text}");
    }
}

#[test]
fn synopsis_pre_delivers_head_and_invisible_rows_before_the_owner_drains() {
    // Both complete inputs reran pinned ASCII/UTF-8/HTML/tree/lint before
    // these assertions. mdoc_term.c::synopsis_pre calls term_vspace after
    // Ft/Vt; term.c:475-497 closes the occupied invisible row separately
    // from its explicit vertical row. An IR drain executes neither event.
    for (date, declarations, expected) in [
        (
            "September 12, 2026",
            ".Ft \\&\n.Ft \\&\n.sp 0\n",
            vec!["x", "", "", "        BODY"],
        ),
        (
            "September 13, 2026",
            ".Vt \\&\n.In stdio.h\n",
            vec!["x", "", "        #include <stdio.h> BODY"],
        ),
    ] {
        let source = format!(
            ".Dd {date}\n.Dt PROBE 1\n.Os\n.Sh SYNOPSIS\n.Bl -tag -width Ds\n.It x\n{declarations}.No BODY\n.El\n"
        );
        let query = load_roff_bytes(source.as_bytes()).unwrap();
        let text = render_query_text(&query);
        let rows = text
            .split_once("SYNOPSIS\n")
            .unwrap()
            .1
            .trim_end_matches('\n')
            .lines()
            .collect::<Vec<_>>();
        assert_eq!(rows, expected, "{source}\n{text}");
        assert_eq!(mant_ir::inline_plain_text(&item(&query).terms[0]), "x");
        assert_eq!(
            item(&query).head_body_relation,
            mant_ir::HeadBodyRelation::Separate
        );
    }
}

#[test]
fn invisible_synopsis_words_keep_each_real_row_across_owner_drains() {
    // All ten complete sources ran pinned ASCII/UTF-8/HTML/tree/lint first.
    // The parser joins two quoted empty operands into one TEXT(" ");
    // term_word buffers that blank, while a lone empty/font word writes
    // no cell. NBRZW and blank-only buffers both
    // reach the ordinary term_flushln tail endline (term.c:233-253), while
    // bare BACKAFTER survives term_newln when lastcol and viscol are zero.
    for (operand, count, expected) in [
        ("\\&", 2, vec!["x", "", "", "        BODY"]),
        ("\\&", 3, vec!["x", "", "", "", "", "        BODY"]),
        ("\\&", 4, vec!["x", "", "", "", "", "", "", "        BODY"]),
        ("\"\"", 2, vec!["x", "", "        BODY"]),
        ("\"\"", 3, vec!["x", "", "", "        BODY"]),
        ("\"\" \"\"", 2, vec!["x", "", "", "        BODY"]),
        ("\"\" \"\"", 3, vec!["x", "", "", "", "", "        BODY"]),
        ("\\fB", 3, vec!["x", "", "", "        BODY"]),
        ("\\z", 3, vec!["x", "", "", "        ODY"]),
        (
            "\\zX",
            3,
            vec![
                "x       X",
                "",
                "        X",
                "",
                "        X",
                "        BODY",
            ],
        ),
    ] {
        let declarations = format!(".Ft {operand}\n").repeat(count);
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh SYNOPSIS\n.Bl -tag -width Ds\n.It x\n{declarations}.sp 0\n.No BODY\n.El\n"
        );
        let text = render_query_text(&load_roff_bytes(source.as_bytes()).unwrap());
        let rows = text
            .split_once("SYNOPSIS\n")
            .unwrap()
            .1
            .trim_end_matches('\n')
            .lines()
            .collect::<Vec<_>>();
        assert_eq!(rows, expected, "{source}\n{text}");
    }
}

#[test]
fn synopsis_blank_word_closes_the_tag_before_counting_vertical_rows() {
    // Every exact variant ran all five fixed CVS profiles. In mdoc_term.c,
    // synopsis_pre() calls term_vspace between two Ft nodes. The first
    // flush closes the still printed tag, then the independent vspace
    // emits a blank row. roff_term_pre_br() closes the next blank-only
    // field after the native printed column has returned to zero.
    let fixture = include_str!("definition_body_breaks/synopsis-blank-word.1");
    for (source, section, blanks) in [
        (fixture.to_owned(), "SYNOPSIS", 2),
        (fixture.replace(".sp 0", ".br"), "SYNOPSIS", 2),
        (fixture.replace(".sp 0", ".sp -1"), "SYNOPSIS", 2),
        (fixture.replace(".sp 0", ".sp 1"), "SYNOPSIS", 3),
        (fixture.replacen(".Ft \"\" \"\"\n", "", 1), "SYNOPSIS", 0),
        (
            fixture.replace(".Sh SYNOPSIS", ".Sh DESCRIPTION"),
            "DESCRIPTION",
            0,
        ),
    ] {
        let query = load_roff_bytes(source.as_bytes()).unwrap();
        let text = render_query_text(&query);
        let (_, body) = text.split_once(&format!("{section}\n")).unwrap();
        let expected = format!("x\n{}        BODY", "\n".repeat(blanks));
        assert_eq!(body.trim_end_matches('\n'), expected, "{source}");
        assert_eq!(
            item(&query).head_body_relation,
            mant_ir::HeadBodyRelation::Separate
        );
    }
}

#[test]
fn pending_body_scan_observes_author_mode_executed_by_the_term() {
    // The fixed CVS renderer executes the It head before the body; -split in
    // the head therefore makes the body An close the pending tag row.
    let source = ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh AUTHORS\n.Bl -tag -width Ds\n.It Xo\n.No x\n.An -split\n.Xc\n.An \\&\n.sp 0\n.No BODY\n.El\n";
    let query = load_roff_bytes(source.as_bytes()).unwrap();
    let text = render_query_text(&query);
    let lines = text.lines().collect::<Vec<_>>();
    let head = lines.iter().position(|line| line.trim() == "x").unwrap();
    let body = lines.iter().position(|line| line.trim() == "BODY").unwrap();
    assert_eq!(body - head - 1, 1, "{text}");
}

#[test]
fn pending_definition_head_executes_no_break_margin_flush_in_order() {
    for (label, source) in [
        (
            "man",
            ".TH PROBE 1\n.SH TEST\n.TP\n.B x\n\\&\\p\n.mc |\nBODY\n",
        ),
        (
            "mdoc-tag",
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bl -tag -width Ds\n.It x\n.No \\&\\p\n.mc |\n.No BODY\n.El\n",
        ),
        (
            "mdoc-hang",
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bl -hang -width Ds\n.It x\n.No \\&\\p\n.mc |\n.No BODY\n.El\n",
        ),
    ] {
        let query = load_roff_bytes(source.as_bytes()).unwrap();
        assert!(item(&query).inline_term(), "{label}: {source}");
        let text = render_query_text(&query);
        assert!(
            text.lines()
                .any(|line| line.trim_start().starts_with('x') && line.contains("BODY")),
            "{label}: {source}\n{text}"
        );
    }
}

#[test]
fn requests_after_printable_body_do_not_retroactively_stack_the_head() {
    for prefix in ["", ".ft B\n", ".PD 0\n", ".ta 4n\n", ".ll 50n\n"] {
        for request in [".br", ".fi", ".nf"] {
            let source =
                format!(".TH PROBE 1\n.SH TEST\n.TP\n.B x\n{prefix}FIRST\n{request}\nSECOND\n");
            let query = load_roff_bytes(source.as_bytes()).unwrap();
            let text = render_query_text(&query);
            assert!(item(&query).inline_term(), "{source}\n{text}");
            assert!(
                text.lines()
                    .any(|line| line.starts_with('x') && line.contains("FIRST")),
                "{source}\n{text}"
            );
            assert!(
                text.lines().any(|line| line.trim() == "SECOND"),
                "{source}\n{text}"
            );
        }
    }
}

#[test]
fn ordinary_fitting_and_mdoc_targeted_definitions_keep_their_existing_contracts() {
    for (head, inline) in [
        (".TP\n.B x\n", true),
        (".TP\n.B longer-than-the-tag-width\n", false),
        (".TP\n.B longer-than-the-tag-width\n.TQ\n.B x\n", true),
    ] {
        let source = format!(".TH PROBE 1\n.SH TEST\n{head}BODY\n");
        let query = load_roff_bytes(source.as_bytes()).unwrap();
        assert_eq!(item(&query).inline_term(), inline, "{source}");
    }
    let query = load_roff_bytes(b".Dd September 11, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bl -tag -width Ds\n.Tg Exact.Target\n.It Fl x\nBODY\n.El\n").unwrap();
    assert!(item(&query).inline_term());
    assert_eq!(
        mant_ir::validate_document(query.document.as_ref().unwrap()).len(),
        0
    );
    let json = serde_json::to_string(query.document.as_ref().unwrap()).unwrap();
    assert!(json.contains("Exact.Target"));
    assert!(render_query_text(&query).contains("BODY"));
}
