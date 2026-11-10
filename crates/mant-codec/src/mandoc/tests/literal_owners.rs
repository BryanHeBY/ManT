//! Completed literal owners are content, not a replayable source program.

use super::{Block, Inline, inline_text, parse_manual_bytes};

const ROW_HEADER: &str =
    ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH DESCRIPTION\n.PD 0\n.nf\n";
const MAN_DISPLAY: &str = ".TH TEST 3\n.SH SYNOPSIS\n.nf\n.B #include <regex.h>\n.P\n.BI \"int regcomp(regex_t *\" preg \");\"\n.sp 2\n.in +4n\n.B typedef struct {\n.B } regex_t;\n.in -4n\n.RS 3\n.B INNER\n.RE\n.B AFTER\n.fi\n.SH DESCRIPTION\nprose\n";
const MDOC_DISPLAY: &str = ".Dd October 15, 2026\n.Dt TEST 3\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh SYNOPSIS\n.Bd -literal -offset indent\nALPHA\n.Pp\nBETA\n.sp 2\nGAMMA\n.Ed\n.Bd -literal -offset indent\nDELTA\n.Ed\n.Sh DESCRIPTION\nprose\n";

fn document(source: &str) -> mant_ir::Document {
    let document = parse_manual_bytes(std::path::Path::new("literal-owner.3"), source.as_bytes())
        .expect("lower completed literal owner");
    let json = serde_json::to_string(&document).expect("serialize completed document");
    let decoded: mant_ir::Document =
        serde_json::from_str(&json).expect("strict document roundtrip");
    assert_eq!(decoded, document);
    assert!(
        !document.diagnostics.iter().any(|diagnostic| diagnostic
            .code
            .as_deref()
            .is_some_and(|code| code.starts_with("ir."))),
        "completed owner remains valid: {document:#?}"
    );
    document
}

#[test]
fn man_paragraphs_spacing_and_relative_offsets_keep_one_literal_owner() {
    // Exact source ran fixed CVS ASCII/UTF-8/HTML/tree/lint first. pre_PP
    // and print_bvspace in man_term.c only end the row and insert PD space;
    // pre_in and pre_RS/post_RS change the offset, not the no-fill channel.
    let document = document(MAN_DISPLAY);
    let [
        Block::Preformatted {
            children,
            inline_layout,
            ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("one nf/fi result owner: {:#?}", document.sections[0].blocks);
    };
    assert_eq!(
        inline_text(children),
        "#include <regex.h>\n\nint regcomp(regex_t *preg);\n\n\ntypedef struct {\n} regex_t;\nINNER\nAFTER"
    );
    for (row, indent) in [(5, 4), (6, 4), (7, 3)] {
        assert_eq!(
            inline_layout.row_layout(row).indent_columns,
            indent,
            "row {row}: {inline_layout:#?}"
        );
    }
    assert_eq!(inline_layout.row_layout(8).indent_columns, 0);
    assert!(
        children
            .iter()
            .any(|inline| matches!(inline, Inline::Strong { .. }))
    );
    assert!(
        children
            .iter()
            .any(|inline| matches!(inline, Inline::Emphasis { .. }))
    );
    assert!(
        matches!(document.sections[1].blocks.as_slice(), [Block::Paragraph { children, .. }] if inline_text(children) == "prose")
    );
}

#[test]
fn separate_no_fill_and_example_owners_do_not_merge_by_adjacency() {
    // Each exact source ran fixed CVS profiles first. fi/nf and EE/EX
    // call term_newln via roff_term_pre_br/man pre_literal, with no gap.
    // Their identical visible rows do not erase the authored display boundary.
    for source in [
        ".TH TEST 3\n.SH SYNOPSIS\n.nf\nALPHA\n.fi\n.nf\nBETA\n.fi\n",
        ".TH TEST 3\n.SH SYNOPSIS\n.EX\nALPHA\n.EE\n.EX\nBETA\n.EE\n",
    ] {
        let document = document(source);
        assert!(
            matches!(document.sections[0].blocks.as_slice(), [Block::Preformatted { children: a, .. }, Block::Preformatted { children: b, .. }] if inline_text(a) == "ALPHA" && inline_text(b) == "BETA"),
            "{document:#?}"
        );
    }
}

#[test]
fn mdoc_paragraphs_and_space_are_inside_their_own_display() {
    // Exact source ran fixed profiles first. termp_pp_pre is term_vspace;
    // roff_term_pre_sp adds its resolved rows, while termp_bd_post closes
    // each Bd BODY independently. The second Bd must remain another owner.
    let document = document(MDOC_DISPLAY);
    let [
        Block::Preformatted {
            children: first, ..
        },
        Block::Preformatted {
            children: second, ..
        },
    ] = document.sections[1].blocks.as_slice()
    else {
        panic!("two authored displays: {:#?}", document.sections[1].blocks);
    };
    assert_eq!(inline_text(first), "ALPHA\n\nBETA\n\n\nGAMMA");
    assert_eq!(inline_text(second), "DELTA");
}

#[test]
fn tagged_semantic_structure_ends_the_surrounding_literal_owner() {
    // Exact source ran fixed profiles first. man pre_TP/post_TP owns
    // the tag and BODY field; the outer no-fill content cannot flatten it.
    let document =
        document(".TH TEST 3\n.SH SYNOPSIS\n.nf\nBEFORE\n.TP\n.B FLAG\nBODY\n.P\nAFTER\n.fi\n");
    assert!(
        matches!(document.sections[0].blocks.as_slice(), [Block::Preformatted { children: before, .. }, Block::DefinitionList { .. }, Block::Preformatted { children: after, .. }] if inline_text(before) == "BEFORE" && inline_text(after) == "AFTER"),
        "{document:#?}"
    );
}

#[test]
fn paragraph_assembly_preserves_authored_link_and_style_identity() {
    // Exact source ran fixed profiles first. man pre_UR visits BODY,
    // post_UR emits <target>; HTML's first eligible label keeps one link
    // annotation. A PP gap does not create or duplicate a link owner.
    let document = document(
        ".TH TEST 3\n.SH SYNOPSIS\n.nf\n.UR https://example.org\n.B FIRST\n.P\n.I SECOND\n.UE\n.fi\n",
    );
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("complete linked display: {document:#?}");
    };
    assert_eq!(
        inline_text(children),
        "FIRST\n\nSECOND <https://example.org>"
    );
    let links = super::control_boundaries::document_link_targets(&document);
    assert_eq!(links.len(), 1, "{document:#?}");
}

#[test]
fn literal_assembly_uses_executed_gaps_and_row_hints_not_requests() {
    // Exact sources ran fixed profiles first. term_vspace consumes negative
    // skipvsp before pre_PP; pre_ti affects just the next physical row.
    let skipped =
        document(".TH TEST 3\n.SH SYNOPSIS\n.nf\nALPHA\n.sp -1\n.P\nBETA\n.sp 2\nGAMMA\n.fi\n");
    assert!(
        matches!(skipped.sections[0].blocks.as_slice(), [Block::Preformatted { children, .. }] if inline_text(children) == "ALPHA\nBETA\n\n\nGAMMA"),
        "{skipped:#?}"
    );
    let indented = document(".TH TEST 3\n.SH SYNOPSIS\n.nf\nALPHA\n.ti +2n\nBETA\nGAMMA\n.fi\n");
    let [
        Block::Preformatted {
            children,
            inline_layout,
            ..
        },
    ] = indented.sections[0].blocks.as_slice()
    else {
        panic!("temporary origin stays in the same owner: {indented:#?}");
    };
    assert_eq!(inline_text(children), "ALPHA\nBETA\nGAMMA");
    assert_eq!(inline_layout.row_layout(1).first_line_indent_columns, 2);
    assert!(inline_layout.row_layout(2).is_empty());
}

#[test]
fn relative_scope_transfer_keeps_nested_fill_boundaries() {
    // Exact source ran fixed profiles first. RS's pre/post change offsets;
    // fi/nf inside its BODY still delimit distinct result owners after RE.
    let document = document(
        ".TH TEST 3\n.SH SYNOPSIS\n.nf\nALPHA\n.RS 3\nBETA\n.fi\n.nf\nGAMMA\n.RE\nDELTA\n.fi\n",
    );
    let [
        Block::Preformatted {
            children: first,
            inline_layout: first_layout,
            ..
        },
        Block::Preformatted {
            children: second,
            inline_layout: second_layout,
            ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("nested fi/nf proof survives RS transfer: {document:#?}");
    };
    assert_eq!(inline_text(first), "ALPHA\nBETA");
    assert_eq!(inline_text(second), "GAMMA\nDELTA");
    assert_eq!(first_layout.row_layout(1).indent_columns, 3);
    assert_eq!(second_layout.row_layout(1).indent_columns, -3);
}

#[test]
fn relative_scope_drains_keep_completed_source_rows_in_one_literal() {
    // All exact sources ran pinned ASCII/UTF-8/HTML/tree/lint. The source
    // gate and pre_RS/post_RS each call guarded term_newln() (man_term.c:
    // 769-823,918-926). Empty TEXT additionally calls term_vspace();
    // its completed endline must not claim a later NBRZW cell's row.
    for (source_first, retained_first) in [
        ("\\&\n", ""),
        ("\n", ""),
        ("\\&   \\&\n", "   "),
        ("\t\n", "\t"),
        ("\\~\n", "\u{a0}"),
    ] {
        for count in 1_usize..=3 {
            let prefix: Vec<_> = std::iter::once(source_first)
                .chain(std::iter::repeat_n("\\&\n", count - 1))
                .collect();
            for index in [0, 1, count] {
                for (open, close) in [
                    ("", ""),
                    (".RS 0\n", ".RE\n"),
                    (".RS 0\n.RS 0\n", ".RE\n.RE\n"),
                ] {
                    for after in ["", "AFTERMARK\n"] {
                        let source = format!(
                            "{ROW_HEADER}{}{open}{}{after}{close}.fi\n",
                            prefix[..index].concat(),
                            prefix[index..].concat(),
                        );
                        let assert_rows = |document: &mant_ir::Document, omit: bool| {
                            let [Block::Preformatted { children, .. }] =
                                document.sections[1].blocks.as_slice()
                            else {
                                panic!("{source}: {:#?}", document.sections[1].blocks);
                            };
                            let mut rows = vec![retained_first];
                            rows.extend(std::iter::repeat_n("", count - 1));
                            if !after.is_empty() {
                                rows.push("AFTERMARK");
                                if omit && count == 1 && retained_first.is_empty() {
                                    rows.remove(0);
                                }
                            }
                            assert_eq!(inline_text(children), rows.join("\n"), "{source}");
                            assert_eq!(
                                mant_ir::logical_row_count(children),
                                rows.len(),
                                "{source}"
                            );
                        };
                        let native = super::super::parse_native_manual(
                            std::path::Path::new("literal-owner.1"),
                            source.as_bytes(),
                        )
                        .unwrap();
                        assert_rows(&native, false);
                        assert_rows(&document(&source), true);
                    }
                }
            }
        }
    }
}

#[test]
fn real_fill_transitions_preserve_all_empty_displays_and_normalize_separately() {
    // These exact matrix members were run against pinned CVS before adding
    // assertions. Unlike RS, fi/nf changes the fill mode and opens a second
    // literal owner (roff_term.c::roff_term_pre_br, man_term.c::pre_literal).
    for (first, expected_first) in [("\\&\n", ""), ("\n", ""), ("\t\n", "\t")] {
        for rest in 1..=2 {
            let source = format!(
                "{ROW_HEADER}{first}.fi\n.nf\n{}AFTERMARK\n.fi\n",
                "\\&\n".repeat(rest)
            );
            let document = document(&source);
            let [
                Block::Preformatted {
                    children: first, ..
                },
                Block::Preformatted {
                    children: second, ..
                },
            ] = document.sections[1].blocks.as_slice()
            else {
                panic!("{source}: {:#?}", document.sections[1].blocks);
            };
            assert_eq!(inline_text(first), expected_first, "{source}");
            assert_eq!(mant_ir::logical_row_count(first), 1, "{source}");
            assert_eq!(
                inline_text(second),
                if rest == 1 {
                    "AFTERMARK"
                } else {
                    "\n\nAFTERMARK"
                },
                "{source}"
            );
        }
    }
}
