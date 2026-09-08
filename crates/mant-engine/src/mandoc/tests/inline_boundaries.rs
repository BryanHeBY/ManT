//! Original mdoc boundary probes, independent of host formatter installation.
use super::*;

fn source(body: &str) -> String {
    format!(
        ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd probe\n.Sh DESCRIPTION\n{body}\n"
    )
}

pub(super) fn query(body: &str) -> ResolvedContent {
    crate::query_roff_bytes(source(body).as_bytes()).unwrap()
}

fn assert_flow(body: &str, expected: &str) {
    let query = query(body);
    let document = query.document.as_ref().unwrap();
    let blocks = &document.sections[1].blocks;
    let inlines = match &blocks[0] {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => children,
        Block::DefinitionList { items, .. } => &items[0].terms[0],
        Block::Table { rows, .. } => match &rows[0].cells[0].blocks[0] {
            Block::Paragraph { children, .. } => children,
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    assert_eq!(inline_text(inlines), expected, "{body}");
    assert!(
        unindent(&crate::render_query_text(&query)).contains(expected),
        "{body}"
    );
    let markdown = crate::render_markdown(&query);
    let reparsed = crate::query_markdown_text(&markdown, None).unwrap();
    assert!(
        unindent(&crate::render_query_text(&reparsed)).contains(expected),
        "{body}: {markdown}"
    );
    assert!(
        mant_ir::validate_document(document).is_empty(),
        "{body}: {:?}",
        document.diagnostics
    );
}

fn unindent(text: &str) -> String {
    text.lines()
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn variants(body: &str) -> Vec<String> {
    vec![
        body.into(),
        format!(".Bd -literal\n{body}\n.Ed"),
        format!(
            ".Bl -tag -width Ds\n.It {}\nDescription.\n.El",
            body.strip_prefix('.').unwrap()
        ),
        format!(".TS\nl.\nT{{\n{body}\nT}}\n.TE"),
    ]
}

#[test]
fn generated_option_prefix_join_ends_with_its_operands() {
    for (body, expected) in [
        (".Fl \"\" Ar file", "- file"),
        (r".Fl \& Ar file", "- file"),
        (".Fl \"\" No next", "- next"),
        (".Fl", "-"),
        (".Fl a", "-a"),
        (".Fl Fl a", "--a"),
        (".Fl Ar file", "-file"),
        (".Fl Fl Ar file", "--file"),
        (".Fl \"\" Ns Ar file", "-file"),
        (".Fl a Ns Ar tail", "-atail"),
        (".Fl \"\" Pf X No y", "- Xy"),
    ] {
        for input in variants(body) {
            assert_flow(&input, expected);
        }
    }
    for operand in ["\"\"", r"\&"] {
        assert_flow(&format!(".Fl {operand}\n.No next"), "- next");
        assert_flow(
            &format!(".Bd -literal\n.Fl {operand}\n.No next\n.Ed"),
            "-\nnext",
        );
        assert_flow(
            &format!(".TS\nl.\nT{{\n.Fl {operand}\n.No next\nT}}\n.TE"),
            "- next",
        );
    }
}

#[test]
fn apostrophes_attach_across_prose_and_style_siblings() {
    for (body, expected) in [
        (".No x Ap y", "x'y"),
        (".Em x Ap y", "x'y"),
        (".Fn execve Ap d", "execve()'d"),
        (".No x Ap Ap y", "x''y"),
        (".No x Ns Ap Ns y", "x'y"),
    ] {
        for input in variants(body) {
            assert_flow(&input, expected);
        }
    }
}

#[test]
fn generated_closers_consume_internal_boundaries_but_not_external_ones() {
    for (open, close, left, right) in [
        ("Oo", "Oc", "[", "]"),
        ("Po", "Pc", "(", ")"),
        ("Bo", "Bc", "[", "]"),
        ("Bro", "Brc", "{", "}"),
        ("Qo", "Qc", "\"", "\""),
        ("Do", "Dc", "“", "”"),
        ("So", "Sc", "‘", "’"),
        ("Ao", "Ac", "<", ">"),
    ] {
        for inner in ["b Ns", "Pf b", "Em b Ns"] {
            for input in variants(&format!(".No a {open} {inner} {close} c")) {
                assert_flow(&input, &format!("a {left}b{right} c"));
            }
        }
    }
    assert_flow(".Op Fl a Ns\n.No b", "[-a]b");
    assert_flow(
        ".Bl -tag -width Ds\n.It Fl x Oo Ar arg Ns Oc Ar tail\nBody.\n.El",
        "-x [arg] tail",
    );
}

#[test]
fn line_start_ns_and_pf_without_same_line_successor_do_not_join_words() {
    // These are recovery cases, not examples of valid inline control usage.
    // Native warnings remain observable instead of weakening the text check.
    for body in ["a\n.Ns\nb", ".Pf a\nb"] {
        assert_flow(body, "a b");
        assert_flow(&format!(".Bd -literal\n{body}\n.Ed"), "a\nb");
        let native = Parser::new(libmandoc_rs::ParseOptions::default())
            .parse_bytes("probe.1", source(body).as_bytes())
            .unwrap();
        let query = query(body);
        for finding in super::super::diagnostics::lower_diagnostics(&native.diagnostics) {
            assert!(
                query
                    .document
                    .as_ref()
                    .unwrap()
                    .diagnostics
                    .contains(&finding)
            );
        }
    }
    for (body, expected) in [(".No a Ns b", "ab"), (".No Pf a b", "ab")] {
        for input in variants(body) {
            assert_flow(&input, expected);
        }
    }
}

#[test]
fn invisible_targets_preserve_pending_joins_and_source_ownership() {
    assert_flow(".No x Ap\n.\\\" hidden comment\n.No y", "x'y");
    // Ordinary words remain Es operands; native parsing can promote bare
    // punctuation into visible siblings, which is a different input tree.
    assert_flow(".No x Ap\n.Es BEGIN END\n.No y", "x'y");
    let body = ".No x Ap\n.Tg Exact.Target\n.Em y";
    assert_flow(body, "x'y");
    let query = query(body);
    let document = query.document.as_ref().unwrap();
    assert!(anchor_ids(document).contains(&"exact-target".into()));
    assert!(
        anchor_owner_lines(document)
            .iter()
            .any(|(id, line)| id == "exact-target" && *line == 10)
    );
    let Block::Paragraph {
        children, source, ..
    } = &document.sections[1].blocks[0]
    else {
        panic!("expected paragraph")
    };
    assert_eq!(source.unwrap().line, 8);
    assert!(children.iter().any(
        |inline| matches!(inline, Inline::Emphasis { children } if inline_text(children) == "y")
    ));
}

#[test]
fn real_breaks_reset_joins_but_source_continuations_do_not() {
    assert_flow(".No x Ap\n.br\n.No y", "x'\ny");
    assert_flow(".No x\\c\n.Em y", "xy");
    let query = query(".No x Ap\n.Pp\n.No y");
    let document = query.document.as_ref().unwrap();
    let paragraphs = document.sections[1]
        .blocks
        .iter()
        .filter(|block| !matches!(block, Block::VerticalSpace { .. }))
        .collect::<Vec<_>>();
    let [
        Block::Paragraph {
            children: first, ..
        },
        Block::Paragraph {
            children: second, ..
        },
    ] = paragraphs.as_slice()
    else {
        panic!("paragraph boundary lost")
    };
    assert_eq!(inline_text(first), "x'");
    assert_eq!(inline_text(second), "y");
}
