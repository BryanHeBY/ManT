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
                assert!(!owner.layout.inline_term, "{source}\n{text}");
                let lines: Vec<_> = text.lines().collect();
                let head_row = lines
                    .iter()
                    .position(|line| line.trim() == "window")
                    .unwrap();
                assert_eq!(lines[head_row + 1].trim(), "BODY", "{source}\n{text}");
                assert!(lines[head_row + 1].starts_with(' '), "{source}\n{text}");
                assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
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
    assert!(!owner.layout.inline_term, "{text}");
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
    assert!(item(&query).layout.inline_term);
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
        assert!(item(&query).layout.inline_term, "{label}: {source}");
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
            assert!(item(&query).layout.inline_term, "{source}\n{text}");
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
        assert_eq!(item(&query).layout.inline_term, inline, "{source}");
    }
    let query = load_roff_bytes(b".Dd September 11, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bl -tag -width Ds\n.Tg Exact.Target\n.It Fl x\nBODY\n.El\n").unwrap();
    assert!(item(&query).layout.inline_term);
    assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
    let json = serde_json::to_string(query.document.as_ref().unwrap()).unwrap();
    assert!(json.contains("Exact.Target"));
    assert!(render_query_text(&query).contains("BODY"));
}
