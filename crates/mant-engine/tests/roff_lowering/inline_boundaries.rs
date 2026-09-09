//! Original mdoc boundary probes, independent of host formatter installation.
use super::*;

fn source(body: &str) -> String {
    format!(
        ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd probe\n.Sh DESCRIPTION\n{body}\n"
    )
}

pub(super) fn query(body: &str) -> ResolvedContent {
    mant_loader::load_roff_bytes(source(body).as_bytes()).unwrap()
}

pub(super) fn assert_flow(body: &str, expected: &str) {
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
        unindent(&mant_render::render_query_text(&query)).contains(expected),
        "{body}"
    );
    let markdown = mant_codec::encode::render_markdown(&query);
    let reparsed = mant_loader::load_markdown_text(&markdown, None).unwrap();
    assert!(
        unindent(&mant_render::render_query_text(&reparsed)).contains(expected),
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
        // The codec-local companion checks each native diagnostic conversion;
        // this public boundary must retain those findings in the query result.
        let expected =
            parse_manual_bytes(std::path::Path::new("probe.1"), source(body).as_bytes()).unwrap();
        let query = query(body);
        for finding in expected.diagnostics {
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

#[test]
fn preserves_complete_mdoc_include_directives() {
    let document = parse_manual_bytes(
        std::path::Path::new("include.3"),
        b".Dd August 19, 2026\n.Dt INCLUDE 3\n.Os\n.Sh SYNOPSIS\n.In fido.h\n",
    )
    .expect("lower mdoc include");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one include paragraph");
    };
    assert_eq!(inline_text(children), "#include <fido.h>");
    assert!(matches!(
        children.as_slice(),
        [Inline::Code { value }] if value == "#include <fido.h>"
    ));
}

#[test]
fn retains_punctuation_after_implicit_mdoc_enclosures() {
    let document = parse_manual_bytes(
        std::path::Path::new("implicit-enclosure-punctuation.7"),
        b".Dd August 19, 2026\n.Dt IMPLICIT-ENCLOSURE-PUNCTUATION 7\n.Os\n\
.Sh DESCRIPTION\nWhen disabled\n.Pq all features remain readable ;\ncontinue safely.\n",
    )
    .expect("lower punctuation after an implicit enclosure");

    let Block::Paragraph { children, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one paragraph");
    };
    assert_eq!(
        inline_text(children),
        "When disabled (all features remain readable); continue safely."
    );
}

#[test]
fn preserves_explicit_mdoc_function_and_enclosure_structure() {
    let document = parse_manual_bytes(
        std::path::Path::new("explicit-mdoc.1"),
        b".Dd August 17, 2026\n\
.Dt EXPLICIT-MDOC 1\n\
.Os\n\
.Sh NAME\n\
.Nm explicit-mdoc\n\
.Nd exercise explicit blocks\n\
.Sh FUNCTION\n\
.Ft int\n\
.Fo audit_open\n\
.Fa \"const char *path\"\n\
.Fa \"int flags\"\n\
.Fc\n\
.Sh ENCLOSURES\n\
.Ao\nangle\n.Ac\n\
.Bo\nbracket\n.Bc\n\
.Do\ndouble\n.Dc\n\
.Po\nparenthesized\n.Pc\n\
.Qo\nquoted\n.Qc\n\
.So\nsingle\n.Sc\n\
.Bro\nbraced\n.Brc\n\
.Oo\noptional\n.Oc\n\
.Eo <<\ngeneric\n.Ec >>\n\
.Es [[ ]]\n\
.En custom\n",
    )
    .expect("lower explicit mdoc blocks");

    let function = &document.sections[1];
    let [
        Block::Paragraph {
            children: declaration,
            ..
        },
    ] = function.blocks.as_slice()
    else {
        panic!("expected one prose function paragraph");
    };
    assert_eq!(
        inline_text(declaration),
        "int audit_open(const char *path, int flags)"
    );
    assert!(declaration.iter().any(|inline| matches!(
        inline,
        Inline::Strong { children } if inline_text(children) == "audit_open"
    )));
    assert!(anchor_ids(&document).iter().any(|id| id == "audit-open"));

    let [Block::Paragraph { children, .. }] = document.sections[2].blocks.as_slice() else {
        panic!("expected one enclosure paragraph");
    };
    assert_eq!(
        inline_text(children),
        "<angle> [bracket] “double” (parenthesized) \"quoted\" ‘single’ {braced} \
         [optional] <<generic>> [[custom]]"
    );
    assert_eq!(document.diagnostics.len(), 2);
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message.starts_with("obsolete macro:")),
        "{:?}",
        document.diagnostics
    );
}

#[test]
fn preserves_the_complete_libbsd_library_identity() {
    let document = parse_manual_bytes(
        std::path::Path::new("libbsd.3bsd"),
        b".Dd August 19, 2026\n.Dt LIBBSD 3bsd\n.Os\n.Sh LIBRARY\n.Lb libbsd\n",
    )
    .expect("lower libbsd library declaration");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one library paragraph");
    };

    assert_eq!(
        inline_text(children),
        "Utility functions from BSD systems (libbsd, -lbsd)"
    );
}

#[test]
fn joins_the_final_mdoc_bibliography_authors() {
    let document = parse_manual_bytes(
        std::path::Path::new("bibliography.3"),
        b".Dd August 19, 2026\n.Dt BIBLIOGRAPHY 3\n.Os\n.Sh SEE ALSO\n\
.Rs\n.%A Bentley, J.L.\n.%A McIlroy, M.D.\n.%T Engineering a Sort Function\n.Re\n",
    )
    .expect("lower mdoc bibliography");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one bibliography paragraph");
    };

    assert_eq!(
        inline_text(children),
        "Bentley, J.L. and McIlroy, M.D. Engineering a Sort Function."
    );
}

#[test]
fn preserves_mdoc_name_and_function_punctuation_by_context() {
    let document = parse_manual_bytes(
        std::path::Path::new("function-punctuation.3"),
        b".Dd August 19, 2026\n.Dt FUNCTION-PUNCTUATION 3\n.Os\n\
.Sh NAME\n.Nm function-punctuation\n.Nd test generated punctuation\n\
.Sh SYNOPSIS\n.Fn compact_call \"int value\"\n\
.Fo explicit_call\n.Fa \"int value\" \"const char *label\"\n.Fc\n\
.Sh DESCRIPTION\nThe\n.Fn prose_call \"int value\"\nfunction.\n",
    )
    .expect("lower mdoc generated punctuation");

    let [Block::Paragraph { children: name, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one NAME paragraph");
    };
    assert_eq!(
        inline_text(name),
        "function-punctuation — test generated punctuation"
    );

    let synopsis = document.sections[1]
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_text(children),
            block => panic!("expected synopsis paragraph, got {block:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        synopsis,
        [
            "compact_call(int value);",
            "explicit_call(int value, const char *label);"
        ]
    );

    let [
        Block::Paragraph {
            children: description,
            ..
        },
    ] = document.sections[2].blocks.as_slice()
    else {
        panic!("expected one DESCRIPTION paragraph");
    };
    assert_eq!(
        inline_text(description),
        "The prose_call(int value) function."
    );
}

#[test]
fn preserves_mdoc_synopsis_declaration_units() {
    let document = parse_manual_bytes(
        std::path::Path::new("synopsis-declarations.3"),
        b".Dd August 19, 2026\n.Dt SYNOPSIS-DECLARATIONS 3\n.Os\n\
.Sh SYNOPSIS\n.In synprobe.h\n.Ft const struct stat *\n\
.Fn synprobe_first \"struct thing *a\"\n.Ft void\n\
.Fo synprobe_second\n.Fa \"struct thing *a\"\n.Fa \"int n\"\n.Fc\n\
.Fn synprobe_third \"int n\"\n",
    )
    .expect("lower mdoc synopsis declarations");

    let rendered = document.sections[0]
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_text(children),
            block => panic!("expected synopsis declaration paragraph, got {block:?}"),
        })
        .collect::<Vec<_>>();

    assert_eq!(
        rendered,
        [
            "#include <synprobe.h>",
            "const struct stat * synprobe_first(struct thing *a);",
            "void synprobe_second(struct thing *a, int n);",
            "synprobe_third(int n);",
        ]
    );
}
