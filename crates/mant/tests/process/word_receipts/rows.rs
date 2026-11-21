//! Accepted rows across words, definition fields and structural boundaries.

use mant_ir::inline_plain_text;

use super::{
    EDITABLE_FIELD, LinkTarget, assert_link, assert_readbacks, description, document_links, render,
    run_text_input, search_visible_word, search_visible_word_with_format, without_sgr,
};

const EMPTY_AT_EOF: &str =
    include_str!("../../../../mant-engine/tests/roff_lowering/word_passes/terminal-empty-at-eof.1");

const EMPTY_BEFORE_PP: &str = include_str!(
    "../../../../mant-engine/tests/roff_lowering/word_passes/terminal-empty-before-pp.1"
);

const EMPTY_LINK_BEFORE_PP: &str = include_str!(
    "../../../../mant-engine/tests/roff_lowering/word_passes/terminal-empty-link-before-pp.1"
);

const EMPTY_BEFORE_SPACE: &str = include_str!(
    "../../../../mant-engine/tests/roff_lowering/word_passes/terminal-empty-before-space.1"
);

#[test]
fn rejected_kept_suffix_does_not_repeat_its_accepted_prefix_endline() {
    // Exact rejected, accepted and unkept controls ran fixed CVS in all
    // five profiles. term.c::term_flushln() ends the accepted field at
    // the first breakpoint, then nbr=0 ends just one empty row. The
    // invisible graph accepted by the first field cannot end another row.
    let rejected = r#".Dd November 10, 2026
.Dt CHECK 1
.Os
.Sh NAME
.Nm check
.Nd word audit
.Sh DESCRIPTION
.Bk -words
.No "ALPHA\zX"
.No "\&\p\: \p\:\&\p\: \zY\p BETA"
.br
.No OMEGA
.Ek
"#;
    let accepted = rejected.replacen(r"\p\:\&", r"\&", 1);
    let unkept = rejected.replace(".Bk -words\n", "").replace(".Ek\n", "");
    for (source, expected) in [
        (rejected, "ALPHAX\n\nOMEGA"),
        (&accepted, "ALPHAX\n\nYBETA\nOMEGA"),
        (&unkept, "ALPHAX\n\nOMEGA"),
    ] {
        assert_readbacks(source, expected, None);
    }
}

#[test]
fn man_link_annotation_keeps_the_native_boundary_between_literal_rows() {
    let header = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- word audit\n.SH DESCRIPTION\n.nf";
    // Fixed CVS accepted the first label row and the subsequent empty pass
    // but rejected the rest of that buffer, including the post_UR() target.
    // term.c::term_flushln() cannot revoke the committed first label row.
    let rejected_word = r"\&\p\: \p BETA";
    for (open, close, address, target) in [
        (
            "UR",
            "UE",
            "https://x.io",
            LinkTarget::External {
                uri: "https://x.io".into(),
            },
        ),
        (
            "MT",
            "ME",
            "u@matrix.test",
            LinkTarget::Email {
                address: "u@matrix.test".into(),
            },
        ),
    ] {
        let rejected = format!(
            "{header}\n.{open} {address}\n.B \"ALPHA\\zX\\p\\: \"\n.B \"{rejected_word}\"\n.{close}\n.fi\n.br\nOMEGA\n"
        );
        let accepted = rejected.replace(rejected_word, "BETA");
        let accepted_text = format!("ALPHAX\nBETA <{address}>\nOMEGA");
        for (source, expected, label) in [
            (&rejected, "ALPHAX\n\n\nOMEGA", "ALPHAX\n\n"),
            (&accepted, accepted_text.as_str(), "ALPHAX\nBETA"),
        ] {
            assert_readbacks(source, expected, None);
            assert_eq!(
                document_links(source, "roff"),
                [(target.clone(), label.into())],
                "{source}"
            );
        }
    }
}

#[test]
fn rewound_literal_spaces_keep_existing_cells_before_word_end_breaks() {
    // Fixed CVS ran these exact words with ASCII, UTF-8, HTML, tree and
    // lint. term.c::encode() advances across old cells for raw SP but
    // keeps their bytes. The next marker can replace the automatic blank
    // or a tight word's still-pending X. Only ETA prints after rearming.
    let source = r#".Dd November 10, 2026
.Dt CHECK 1
.Os
.Sh NAME
.Nm check
.Nd word audit
.Sh DESCRIPTION
.No "ALPHA\zX"
.No "\h'-1n'\z \p BETA"
.No ""
.br
.No OMEGA
"#;
    let word = r"\h'-1n'\z \p BETA";
    let same_line = source.replace("\"\n.No \"\\h", "\" No \"\\h");
    let joined = source.replace("\"\n.No \"\\h", "\" Ns No \"\\h");
    let styled = source.replace(".No \"\\h", ".Em \"\\h");
    for (input, expected) in [
        (source.to_owned(), "ALPHAX\nETA\nOMEGA"),
        (same_line, "ALPHAX\nETA\nOMEGA"),
        (joined, "ALPHA\nETA\nOMEGA"),
        (styled, "ALPHAX\nETA\nOMEGA"),
        (
            source.replace(word, r"\h'-1n' \p BETA"),
            "ALPHAX\nBETA\nOMEGA",
        ),
        (
            source.replace(word, "\\h'-1n'\\z\t\\p BETA"),
            "ALPHAX\nETA\nOMEGA",
        ),
        (
            source.replace(word, r"\h'-1n'\z  \p BETA"),
            "ALPHAX\n\nOMEGA",
        ),
        (source.replace(r"\h'-1n'", ""), "ALPHAX  ETA\nOMEGA"),
        (
            source.replace("\n.No \"\"\n.br\n.No OMEGA", ""),
            "ALPHAX\nETA",
        ),
    ] {
        assert_readbacks(&input, expected, None);
    }
    // The tight rewind can remove the pending combining mark while
    // leaving its preceding native backline. A graph on that same cell
    // covers the base only when it prints on the same accepted pass.
    let combining = source
        .replace("\"\n.No \"\\h", "\" Ns No \"\\h")
        .replace(r"ALPHA\zX", "ALPHA\\ze\\z\u{301}");
    for (input, expected) in [
        (combining.clone(), "ALPHAe\nETA\nOMEGA"),
        (combining.replace(r"\z \p BETA", " B"), "ALPHAB\nOMEGA"),
        (
            combining.replace(r"\z \p BETA", r" B\p TAIL"),
            "ALPHAB\nTAIL\nOMEGA",
        ),
        (combining.replace(word, " B"), "ALPHA\u{301}B\nOMEGA"),
    ] {
        assert_readbacks(&input, expected, None);
    }
}

#[test]
fn body_flush_returns_only_printed_blanks_to_a_detached_linked_term() {
    // Both exact inputs ran fixed CVS. mdoc_term.c::termp_it_post() leaves
    // inset HEAD cells live until the BODY flush; term_field() advances
    // authored spaces only when the accepted pass prints them.
    let source = r#".Dd November 10, 2026
.Dt CHECK 1
.Os
.Sh NAME
.Nm check
.Nd word audit
.Sh DESCRIPTION
.Bl -inset -compact
.It Xo
.No "ALPHA\zX"
.Lk https://x.io "\&\p\: \zY\p \&\p\: \p BETA"
.No ""
.Xc
.br
.No OMEGA
.El
"#;
    let overwritten = source.replace(r"\&\p\: \p BETA", r"\&\p\:\fB \p\fIBETA");
    for (input, word) in [(source, " BETA"), (&overwritten, "BETA")] {
        let label = format!("\nY\n\n{word}");
        assert_link(input, "roff", &label);
        let direct = render(input, "roff", "text", "never", false);
        assert_eq!(
            description(&direct),
            format!("ALPHAX\nY\n\n{word}:\nhttps://x.io\nOMEGA")
        );
        assert_eq!(
            without_sgr(&render(input, "roff", "text", "always", false)),
            direct
        );
        for addressable in [false, true] {
            let markdown = render(input, "roff", "markdown", "never", addressable);
            assert_link(&markdown, "markdown", &label);
            let reexport = render(&markdown, "markdown", "markdown", "never", addressable);
            assert_link(&reexport, "markdown", &label);
        }
    }
}

#[test]
fn a_detached_head_does_not_replay_its_endline_in_a_rejecting_body_flush() {
    // The exact sources ran through fixed CVS. mdoc_term.c::termp_it_post()
    // leaves the inset HEAD's cells pending. The first body flush accepts
    // HEAD X, rejects Y, and leaves one actual empty row; a positive sp
    // adds its own row. The accepted HEAD endline closes the term once.
    let source = r#".Dd September 28, 2026
.Dt TEST 1
.Os
.Sh NAME
.Nm test
.Nd probe
.Sh DESCRIPTION
.Bl -inset
.It Xo
.No "X\p \p Y"
.Xc
.br
.No BodyWord
.El
"#;
    for (input, expected, markdown_expected) in [
        (source.to_owned(), "X\n\nBodyWord", "• X\n\n  BodyWord"),
        (
            source.replace(".br\n", ".sp 0\n"),
            "X\n\nBodyWord",
            "• X\n\n  BodyWord",
        ),
        (
            source.replace(".br\n", ".sp 1\n"),
            "X\n\n\nBodyWord",
            "• X\n\n  BodyWord",
        ),
        (
            source.replace(".No \"X", ".Em \"X"),
            "X\n\nBodyWord",
            "• X\n\n  BodyWord",
        ),
        (source.replace(".br\n", ""), "X\n", "• X\n"),
    ] {
        let plain = render(&input, "roff", "text", "never", false);
        assert_eq!(description(&plain), expected, "{input}");
        assert_eq!(
            without_sgr(&render(&input, "roff", "text", "always", false)),
            plain
        );
        let wire = render(&input, "roff", "json", "never", false);
        let bundle: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let document = bundle.document.unwrap();
        let [mant_ir::Block::DefinitionList { items, .. }] = document.sections[1].blocks.as_slice()
        else {
            panic!("expected one definition after JSON transport");
        };
        assert_eq!(inline_plain_text(&items[0].terms[0].content), "X");

        // Raw addressable HTML is outside the semantic reimport subset.
        let mut markdown = render(&input, "roff", "markdown", "never", false);
        for _ in 0..2 {
            let readback = render(&markdown, "markdown", "text", "never", false);
            assert_eq!(description(&readback), markdown_expected, "{markdown}");
            markdown = render(&markdown, "markdown", "markdown", "never", false);
        }
    }
}

#[test]
fn accepted_empty_word_passes_keep_hard_rows_before_a_successor_or_eof() {
    // These exact inputs ran all five pristine profiles. term.c:205–220
    // closes each accepted loop pass, including NBRZW with no printed
    // scalar. The last rejected pass also ends its row (250–253). The
    // following .Pp independently executes term_vspace().
    let before_break = include_str!(
        "../../../../mant-engine/tests/roff_lowering/word_passes/empty-passes-before-break.1"
    );
    let at_eof = include_str!(
        "../../../../mant-engine/tests/roff_lowering/word_passes/empty-passes-at-eof.1"
    );
    let kept = include_str!(
        "../../../../mant-engine/tests/roff_lowering/word_passes/empty-passes-in-keep.1"
    );
    let literal_at_eof = at_eof.replace(".Sh DESCRIPTION\n", ".Sh DESCRIPTION\n.nf\n") + ".fi\n";
    let before_paragraph = before_break.replace(".br\n", ".Pp\n");
    let one_less_pass = before_break.replacen("\\&\\p\\: ", "", 1);
    for (source, expected) in [
        (before_break, "ALPHAX\n\n\nOMEGA"),
        (at_eof, "ALPHAX\nY\n\n\n"),
        (&literal_at_eof, "ALPHAX\nY\n\n\n"),
        (&before_paragraph, "ALPHAX\n\n\n\nOMEGA"),
        (&one_less_pass, "ALPHAX\n\nOMEGA"),
        (kept, "ALPHA\n\n\nOMEGA"),
    ] {
        assert_readbacks(source, expected, None);
    }
}

#[test]
fn deferred_source_space_survives_only_in_the_printed_native_pass() {
    // The pinned terminal prints the SP following an accepted breakpoint.
    // In the negative control encode1() emits a backspace instead of
    // retreating over an immediately preceding SP (term.c:901-908).
    for (source, expected, label) in [
        (
            include_str!(
                "../../../../mant-engine/tests/roff_lowering/word_passes/deferred-source-blank.1"
            ),
            "ALPHAX\nY\n\n BETA\nOMEGA",
            None,
        ),
        (
            include_str!(
                "../../../../mant-engine/tests/roff_lowering/word_passes/deferred-source-blank-in-style.1"
            ),
            "ALPHAX\nY\n\n BETA\nOMEGA",
            None,
        ),
        (
            include_str!(
                "../../../../mant-engine/tests/roff_lowering/word_passes/deferred-source-blank-in-link.1"
            ),
            "ALPHAX\nY\n\n BETA:\nhttps://x.io\nOMEGA",
            Some("\nY\n\n BETA"),
        ),
        (
            include_str!(
                "../../../../mant-engine/tests/roff_lowering/word_passes/overwritten-source-blank.1"
            ),
            "ALPHAX\n\nBETA\nOMEGA",
            None,
        ),
    ] {
        assert_readbacks(source, expected, label);
    }
}

#[test]
fn rejected_plain_tail_and_positive_spacing_keep_independent_rows() {
    // In pinned term.c:143-146,205-220,250-253, the flush closes its
    // accepted pass and then its empty rejected tail. A following .Pp
    // independently runs term_vspace() via mdoc_term.c::termp_pp_pre().
    for (source, expected, label) in [
        (EMPTY_AT_EOF, "ALPHAX\n", None),
        (EMPTY_BEFORE_PP, "ALPHAX\n\n\nOMEGA", None),
        (EMPTY_LINK_BEFORE_PP, "ALPHAX\n\n\nOMEGA", Some("")),
    ] {
        assert_readbacks(source, expected, label);
    }

    // Markdown can simplify a positive .sp 2 to one layout row. The
    // preceding hard empty row must survive that simplification.
    assert_eq!(
        description(&render(EMPTY_BEFORE_SPACE, "roff", "text", "never", false)),
        "ALPHAX\n\n\n\nOMEGA"
    );
    let json = render(EMPTY_BEFORE_SPACE, "roff", "json", "never", false);
    let bundle: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let document = bundle.document.unwrap();
    let section = document.sections.last().unwrap();
    let [
        mant_ir::Block::Paragraph { children, .. },
        mant_ir::Block::VerticalSpace { lines: 2, .. },
        mant_ir::Block::Paragraph { .. },
    ] = section.blocks.as_slice()
    else {
        panic!("expected hard rows and independent .sp 2: {section:?}");
    };
    assert_eq!(inline_plain_text(children), "ALPHAX\n\n");
    for addressable in [false, true] {
        let mut input = render(EMPTY_BEFORE_SPACE, "roff", "markdown", "never", addressable);
        for _ in 0..2 {
            assert_eq!(
                description(&render(&input, "markdown", "text", "never", false)),
                "ALPHAX\n\n\nOMEGA"
            );
            input = render(&input, "markdown", "markdown", "never", addressable);
        }
    }
}

#[test]
fn retained_cell_receipts_survive_both_definition_owners() {
    // The It HEAD and BODY flush separately. Both accept the restored T
    // after A's word-end break (mdoc_term.c::termp_it_post, term.c::term_field).
    let word = r#"No "\zQ\p" Ns No A No "\pT" Ns No "\h'-2n'Y""#;
    for kind in ["tag", "hang"] {
        let head = format!("{EDITABLE_FIELD}.Bl -{kind} -width 12n\n.It {word}\nBODYWORD\n.El\n");
        let body = format!("{EDITABLE_FIELD}.Bl -{kind} -width 12n\n.It alpha\n.{word}\n.El\n");
        let header_tail = if kind == "tag" {
            "A\nYT\nBODYWORD"
        } else {
            "A\nYT BODYWORD"
        };
        for (source, id, expected) in [
            (&head, "term-a-yt", header_tail),
            (&body, "term-alpha", "alpha         A\nYT"),
        ] {
            let json = render(source, "roff", "json", "never", false);
            let document: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(
                document["document"]["sections"][1]["blocks"][0]["items"][0]["entry"]["id"],
                id
            );
            let reading = render(source, "roff", "text", "never", false);
            let rows: Vec<_> = description(&reading).lines().map(str::trim_start).collect();
            assert_eq!(rows, expected.lines().collect::<Vec<_>>(), "{source}");
        }
        let explanation = run_text_input(
            &[
                "--input",
                "-",
                "--input-format",
                "roff",
                "--format",
                "json",
                "--explain",
                "alpha",
            ],
            &body,
        );
        assert!(explanation.status.success(), "{explanation:?}");
        let result: serde_json::Value = serde_json::from_slice(&explanation.stdout).unwrap();
        assert_eq!(result["counts"]["directEntry"]["total"], 1);
        assert_eq!(search_visible_word(&body, "YT"), 1);
    }
}

#[test]
fn section_transitions_publish_only_rows_completed_by_the_device() {
    // pre_SH uses TERMP_NOBREAK with trailspace equal to the field width.
    // A zero-width tail that fits never reaches endline in term_flushln.
    let head = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n";
    let tail = "\nGG\n.PD 1\n.SH ENDTEST\nZZ\n";
    for (request, id, heading, compiled_gap) in [
        ("SH", "section", r"\&", 0),
        ("SH", "p-dd", r"\p DD", 0),
        ("SH", "p-p-dd", r"\p\:\&\p DD", 0),
        ("SS", "p-dd", r"\p DD", 1),
    ] {
        for (pd, mode) in [(0, ""), (0, ".nf\n"), (1, "")] {
            let source = format!("{head}.PD {pd}\n{mode}AA\n.{request} {heading}{tail}");
            let bundle: mant_protocol::QueryBundle =
                serde_json::from_str(&render(&source, "roff", "json", "never", false)).unwrap();
            let document = bundle.document.unwrap();
            let section = if request == "SH" {
                &document.sections[2]
            } else {
                &document.sections[1].children[0]
            };
            let gap = pd + compiled_gap;
            assert_eq!(section.id.as_str(), id, "{source}");
            assert_eq!(section.heading.plain_text(), "", "{source}");
            assert_eq!(usize::from(section.spacing_before_lines), gap, "{source}");
            let output = render(&source, "roff", "text", "never", false);
            let between = output.split_once("\nFIRST\n").unwrap().1;
            let rows = between.split_once("\nENDTEST").unwrap().0.trim_end();
            let rows: Vec<_> = rows.lines().map(str::trim_start).collect();
            let expected = format!("AA\n{}GG", "\n".repeat(gap));
            assert_eq!(rows, expected.lines().collect::<Vec<_>>(), "{source}");
            let selector = format!("id:{id}");
            let excerpt = run_text_input(
                &[
                    "--input",
                    "-",
                    "--input-format",
                    "roff",
                    "--format",
                    "json",
                    "--node",
                    &selector,
                ],
                &source,
            );
            assert!(excerpt.status.success(), "{excerpt:?}");
            let excerpt: mant_protocol::QueryExcerpt =
                serde_json::from_slice(&excerpt.stdout).unwrap();
            let [
                mant_protocol::ExcerptSelection::DocumentSection {
                    section: selected, ..
                },
            ] = excerpt.selections.as_slice()
            else {
                panic!("expected one selected section for {source}");
            };
            assert_eq!(selected.id, section.id);
            assert_eq!(selected.heading, section.heading);
            assert_eq!(selected.spacing_before_lines, section.spacing_before_lines);
        }
    }
}

#[test]
fn a_breakpoint_keeps_the_pending_link_in_its_definition_owner() {
    let word = r#".No "\X'tty: link https://one.test'\zA\p\X'tty: link'" Ns No "\:C""#;
    for kind in ["tag", "hang"] {
        let head = format!(
            "{EDITABLE_FIELD}.Bl -{kind} -width 12n\n.It {}\nBODYWORD\n.El\n",
            &word[1..]
        );
        let body = format!("{EDITABLE_FIELD}.Bl -{kind} -width 12n\n.It alpha\n{word}\n.El\n");
        for (source, id, term, description) in [
            (&head, "term-a-c", "A\nC", "BODYWORD"),
            (&body, "term-alpha", "alpha", "A\nC"),
        ] {
            let bundle: mant_protocol::QueryBundle =
                serde_json::from_str(&render(source, "roff", "json", "never", false)).unwrap();
            let document = bundle.document.unwrap();
            let mant_ir::Block::DefinitionList { items, .. } = &document.sections[1].blocks[0]
            else {
                panic!("{source}");
            };
            let item = &items[0];
            let mant_ir::Block::Paragraph { children, .. } = &item.description[0] else {
                panic!("{source}");
            };
            assert_eq!(item.entry.as_ref().unwrap().id.as_str(), id, "{source}");
            assert_eq!(mant_ir::inline_plain_text(&item.terms[0]), term, "{source}");
            assert_eq!(
                mant_ir::inline_plain_text(children),
                description,
                "{source}"
            );
            assert_eq!(
                document_links(source, "roff"),
                [(
                    LinkTarget::External {
                        uri: "https://one.test".into()
                    },
                    "A".into()
                )]
            );
        }
    }
}

#[test]
fn alternating_fonts_place_a_delayed_glyph_before_the_next_owners_break() {
    // man_term.c::pre_alternate executes separate words without a space.
    // term_fill accepts both graphs before ASCII_BREAK, even though the
    // second glyph only enters the projection with the third operand.
    let head = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH DESCRIPTION\n";
    for style in ["BR", "BI"] {
        for control in ["", r"\&"] {
            let source = format!(r#"{head}.{style} "\pB" "{control}\zD" "\:C""#) + "\n";
            assert_readbacks(&source, "BD\nC", None);
            assert_eq!(search_visible_word(&source, "BD"), 1, "{source}");
            assert_eq!(search_visible_word(&source, "DC"), 0, "{source}");
        }
    }
}

#[test]
fn consecutive_and_detached_man_fields_share_the_device_minimum() {
    // term_flushln assigns minbl from trailspace even when no glyph
    // reaches term_field. An endline consumes that minimum.
    let head = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n";
    for (pd, predecessor, heading, count, gap) in [
        (0, "", r".SH \&", 1, 0),
        (0, "", r".SH \&", 2, 1),
        (0, "", r".SH \&", 3, 1),
        (0, ".IP \\&\n", r".SH \&", 1, 1),
        (0, ".TP\n\\&\n", r".SH \&", 1, 1),
        (0, ".HP\n\\&\n", r".SH \&", 1, 1),
        (1, ".IP \\&\n", r".SH \&", 1, 2),
        (0, ".sp 1\n", r".SH \&", 1, 1),
        (0, "", r".SS \~", 1, 0),
        (0, "", r".SS \0", 1, 0),
        (0, "", r".SH \0", 1, 1),
        (0, "", r".SS \p DD", 1, 1),
    ] {
        let headings = format!("{heading}\n").repeat(count);
        let source =
            format!("{head}.PD {pd}\nAA\n{predecessor}{headings}.PD 0\nDD\n.SH ENDTEST\nZZ\n");
        let full = render(&source, "roff", "text", "never", false);
        let actual = full.split_once("\nFIRST\n").unwrap().1;
        let actual = actual.split_once("\nENDTEST").unwrap().0.trim_end();
        let rows: Vec<_> = actual.lines().map(str::trim).collect();
        let expected = format!("AA\n{}DD", "\n".repeat(gap));
        assert_eq!(rows, expected.lines().collect::<Vec<_>>(), "{source}");
    }
}

#[test]
fn subsection_excerpts_keep_only_the_heading_owned_hard_rows() {
    // man_term.c::pre_SS with PD=0 leaves the old field live. A pass
    // ending the old owner's row must consume its padding without
    // appearing as a leading hard row in the new heading.
    let head = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n.PD 0\n";
    let tail = "\nGG\n.PD 1\n.SH ENDTEST\nZZ\n";
    for (word, heading, id, rows, title) in [
        (r"AA\p\~BB", "DD", "dd", "AA\u{a0}BB\nDD\nGG", "DD"),
        (r"AA\p\:BB", r"\&\p DD", "p-dd", "AA\nBB\nDD\nGG", "DD"),
        (r"AA\p BB", r"\&\p DD", "p-dd", "AA\nBB\nDD\nGG", "DD"),
        (
            r"AA\p\~BB",
            r"\&\p DD",
            "p-dd",
            "AA\u{a0}BB\n\nDD\nGG",
            "\nDD",
        ),
    ] {
        let source = format!("{head}{word}\n.SS {heading}{tail}");
        let bundle: mant_protocol::QueryBundle =
            serde_json::from_str(&render(&source, "roff", "json", "never", false)).unwrap();
        let section = &bundle.document.unwrap().sections[1].children[0];
        assert_eq!(section.heading.plain_text(), title, "{source}");
        let full = render(&source, "roff", "text", "never", false);
        let between = full.split_once("\nFIRST\n").unwrap().1;
        let body = between.split_once("\nENDTEST").unwrap().0.trim_end();
        assert_eq!(
            body.lines().map(str::trim_start).collect::<Vec<_>>(),
            rows.lines().collect::<Vec<_>>(),
            "{source}"
        );
        let selector = format!("id:{id}");
        let excerpt = run_text_input(
            &[
                "--input",
                "-",
                "--input-format",
                "roff",
                "--format",
                "text",
                "--display",
                "direct",
                "--color",
                "never",
                "--node",
                &selector,
            ],
            &source,
        );
        assert!(excerpt.status.success(), "{source}: {excerpt:?}");
        let excerpt = String::from_utf8(excerpt.stdout).unwrap();
        assert_eq!(
            excerpt.splitn(3, "\n\n").last().unwrap().trim_end(),
            format!("{title}\nGG"),
            "{source}"
        );
        if title == "DD" {
            let markdown = render(&source, "roff", "markdown", "never", false);
            assert!(markdown.lines().any(|row| row == "### DD"), "{markdown}");
        }
    }
}

#[test]
fn subsection_padding_shared_with_the_previous_row_is_not_a_heading_row() {
    // man_term.c::pre_SS and term.c::term_flushln share the still open
    // row with the heading's first accepted padding pass at PD=0.
    // A new SH executes separately, and a later blank pass remains.
    let head = ".TH TOOL 1\n.SH DESCRIPTION\n.PD 0\nAA\n";
    for (request, tail, expected) in [
        ("SS", "B", &["AA", "B", "DD"][..]),
        ("SS", r"\0", &["AA", "", "DD"][..]),
        ("SH", "B", &["AA", "", "B", "DD"][..]),
        ("SH", r"\0", &["AA", "", "", "DD"][..]),
    ] {
        let source = format!("{head}.{request} \\~\\p {tail}\nDD\n.SH ENDPROBE\nzz\n");
        let text = render(&source, "roff", "text", "never", false);
        let body = text
            .split_once("DESCRIPTION\n")
            .unwrap()
            .1
            .split_once("\nENDPROBE")
            .unwrap()
            .0;
        assert_eq!(
            body.trim_end_matches('\n')
                .lines()
                .map(str::trim)
                .collect::<Vec<_>>(),
            expected,
            "{source}"
        );
        if request == "SS" {
            let bundle: mant_protocol::QueryBundle =
                serde_json::from_str(&render(&source, "roff", "json", "never", false)).unwrap();
            let document = bundle.document.unwrap();
            let heading = &document.sections[0].children[0].heading;
            assert_eq!(
                heading.plain_text(),
                if tail == "B" { "B" } else { "\u{a0}" }
            );
        }
    }
}

#[test]
fn subsection_padding_can_share_a_row_already_presented_by_a_hanging_body() {
    // CVS post_HP(BODY) calls term_newln with NOBREAK still active.
    // A fitting field prints its cells and leaves viscol open for pre_SS.
    let head = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH CASE0CASE\n.PD 0\nAA\n.HP 4n\n\\&\n\\~\n";
    let tail = "\nDD\n.PD 0\n.SH ENDTEST\nZZ\n";
    for (heading, gaps) in [
        (r".SS \0", 1),
        (r".SS \~", 1),
        (r".SS \p \~", 1),
        (r".SS \&\p \~", 2),
        (r".SS \~\p \0", 2),
        (r".SS \~\p\:\0", 2),
        (".SS \\~\n.SH \\&", 1),
    ] {
        let source = format!("{head}{heading}{tail}");
        let text = render(&source, "roff", "text", "never", false);
        let between = text
            .split_once("CASE0CASE\n")
            .unwrap()
            .1
            .split_once("\nENDTEST")
            .unwrap()
            .0
            .trim_end_matches('\n');
        let expected = format!("AA\n{}DD", "\n".repeat(gaps));
        assert_eq!(
            between.lines().map(str::trim).collect::<Vec<_>>(),
            expected.lines().collect::<Vec<_>>(),
            "{source}"
        );
    }
}

#[test]
fn table_word_end_breaks_reach_their_accepted_interior_row() {
    // tbl_term.c::tbl_word buffers the word before term_fill. BACKBEFORE
    // consumes one SP; two SPs leave the first one to end the current pass.
    for prefix in ["", r"\z"] {
        for blanks in 0..=4 {
            for continued in [false, true] {
                let word = format!("{prefix}A\\p{}C", " ".repeat(blanks));
                let cell = if continued {
                    format!("T{{\n{word}\nT}}")
                } else {
                    word
                };
                let source = format!(
                    ".TH TOOL 1\n.SH DESCRIPTION\n.TS\nlw(20n).\n{cell}\nzzzzzzzzzzzzzzzz\n.TE\n.SH ENDPROBE\nzz\n"
                );
                let expected = match (prefix.is_empty(), blanks) {
                    (false, 0) => "C",
                    (false, 1) | (true, 0) => "AC",
                    _ => "A\nC",
                };
                let markdown = render(&source, "roff", "markdown", "never", false);
                for (input, format) in [(source.as_str(), "roff"), (markdown.as_str(), "markdown")]
                {
                    let text = render(input, format, "text", "never", false);
                    let body = text
                        .split_once("DESCRIPTION\n")
                        .unwrap()
                        .1
                        .split_once("\nENDPROBE")
                        .unwrap()
                        .0
                        .trim_end_matches('\n');
                    assert_eq!(
                        body.lines().map(str::trim).collect::<Vec<_>>(),
                        expected
                            .lines()
                            .chain(["zzzzzzzzzzzzzzzz"])
                            .collect::<Vec<_>>(),
                        "{source}"
                    );
                    assert_eq!(
                        search_visible_word_with_format(input, format, "AC"),
                        u64::from(expected == "AC")
                    );
                }
            }
        }
    }
}
