use super::*;

// The selected UTF-8 formatter preserves CVS's executed glyphs: mdoc Ao/Ac
// select the Unicode angle glyphs, while man post_UR always calls term_word
// with literal ASCII "<"/">" (man_term.c:896-908). These assertions were
// rechecked against pristine ASCII/UTF-8/tree/lint before synchronizing them.

#[test]
fn filled_and_no_fill_segments_share_one_pending_text_execution() {
    // This exact source was run through the fixed CVS -Tutf8/-Tlint oracle.
    // mdoc_term.c::print_mdoc_node() changes the fill channel at NODE_LINE,
    // while term.c::term_word() keeps the backtracking and word-end registers
    // in the same termp across the output destinations.
    let source = b".Dd September 28, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd shared text execution\n.Sh DESCRIPTION\n.No BEFORE\\zX\\c\n.nf\n.No NEXT\n.No SECOND\\p\n.fi\n.No AFTER\n";
    let document = parse_manual_bytes(std::path::Path::new("shared-fill-state.1"), source)
        .expect("lower fill changes with one formatter");
    let blocks = &document.sections[1].blocks;
    assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children) == "BEFOREX")), "{blocks:#?}");
    assert!(blocks.iter().any(|block| matches!(block, Block::Preformatted { children, .. } if inline_text(children).contains("NEXT\nSECOND"))), "{blocks:#?}");
    assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children) == "AFTER")), "{blocks:#?}");
}

#[test]
fn aligned_no_fill_rows_settle_pending_glyphs_at_the_native_flush() {
    // Every exact input was run through the fixed CVS -Tutf8/-Tlint oracle.
    // roff_term.c::roff_term_pre_ce() flushes each grouped line; its .br
    // child calls roff_term_pre_br() and therefore term_newln() first.
    for (name, request, tail, expected) in [
        ("center", ".ce 1", "", "X"),
        ("right", ".rj 1", "", "X"),
        ("center-break", ".ce 2", ".br\nY\n", "XY"),
    ] {
        let source = format!(
            ".TH PROBE 1 \"September 28, 2026\"\n.SH DESCRIPTION\n.nf\n{request}\n\\zX\n{tail}.fi\nAFTER\n"
        );
        let document = parse_manual_bytes(std::path::Path::new(name), source.as_bytes())
            .expect("lower aligned no-fill row");
        let blocks = &document.sections[0].blocks;
        let literal_text = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Preformatted { children, .. } => Some(inline_text(children)),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(
            literal_text.replace('\n', ""),
            expected,
            "{name}: {blocks:#?}"
        );
        assert!(blocks.iter().any(|block| matches!(block, Block::Paragraph { children, .. } if inline_text(children) == "AFTER")), "{name}: {blocks:#?}");
    }
}

#[test]
fn mdoc_explicit_body_end_executes_post_once_at_the_marker() {
    // Exact sources checked with the pinned CVS -Ttree, -Thtml, and -Tutf8.
    // mdoc.c::mdoc_endbody_alloc links the marker to the original BODY;
    // mdoc_html.c::print_mdoc_node executes marker children before its post
    // and sets NODE_ENDED so the original BODY cannot post a second time.
    for (label, source, expected) in [
        (
            "angle",
            ".Dd September 27, 2026\n.Dt CLOSE 1\n.Os\n.Sh DESCRIPTION\n.Ao\n.Bo\ninside\n.Ac\nafter-angle\n.Bc\ntail\n",
            "⟨[inside⟩ after-angle] tail",
        ),
        (
            "authored",
            ".Dd September 27, 2026\n.Dt EC 1\n.Os\n.Sh DESCRIPTION\n.Eo opening\n.Bo\ninside\n.Ec closing\nafter-ec\n.Bc\n",
            "opening[insideclosing after-ec]",
        ),
        (
            "function",
            ".Dd September 28, 2026\n.Dt CLOSE 1\n.Os\n.Sh DESCRIPTION\n.Fo call\n.Bo\n.Fa arg\n.Fc\nafter-fc\n.Bc\nnext\n",
            "call([arg) after-fc] next",
        ),
        (
            "authored-nospace",
            ".Dd September 27, 2026\n.Dt EONS 1\n.Os\n.Sh DESCRIPTION\n.Eo opening\n.Bk -words\ninside\n.Ec closing\n.No pre Ns\n.Ek\n.No NEXT\n",
            "openinginsideclosing preNEXT",
        ),
    ] {
        let document = parse_manual_bytes(std::path::Path::new(label), source.as_bytes())
            .expect("lower explicit mdoc BODY close");
        let text = projected_document_text(&document);
        assert_eq!(
            text.trim(),
            format!("DESCRIPTION{expected}"),
            "{label}: {text:?}"
        );
    }
}

#[test]
fn an_empty_word_keeps_its_executed_gap_before_ns() {
    // Exact input checked against fixed CVS tree, HTML and UTF-8 output.
    // term.c::term_word() has committed the empty operand's separator before
    // mdoc Ns tightens only the following boundary.
    let source = b".Dd September 27, 2026\n.Dt EMPTYJOIN 1\n.Os\n.Sh DESCRIPTION\n.No before No \"\" Ns No after\n";
    let document = parse_manual_bytes(std::path::Path::new("empty-join.1"), source)
        .expect("lower empty word before Ns");
    assert_eq!(
        projected_document_text(&document),
        "DESCRIPTIONbefore after"
    );
}

#[test]
fn formatter_requests_use_the_current_cell_not_prior_document_output() {
    for (label, request) in [("break", ".br"), ("indent", ".ti 4n")] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \\p\n{request}\n.No B C\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("formatter-leading-break-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse leading buffered word-end break");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), "\nB C", "{label}: {children:?}");
    }

    for (label, first, expected) in [
        ("pending", r"\zX", "X B"),
        ("continued-pending", r"\zX\c", "X B"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {first}\n.mc |\n.No B\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("formatter-zero-cell-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse zero-advance formatter cell");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }

    let document = parse_manual_bytes(
        std::path::Path::new("formatter-transparent-target.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No A\n.ti 4n\n.Tg mark\n.ti 4n\n.No B\n",
    )
    .expect("parse formatter boundaries around a transparent target");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "unexpected target-boundary blocks: {:#?}",
            document.sections
        );
    };
    assert_eq!(inline_text(children), "A\nB", "{children:?}");
    assert_eq!(
        children
            .iter()
            .filter(|node| matches!(node, Inline::LineBreak { .. }))
            .count(),
        1,
        "{children:?}"
    );
    assert!(children.iter().any(|node| matches!(
        node,
        Inline::Anchor { id, .. } if id == "mark"
    )));
}

#[test]
fn vertical_space_uses_the_cvs_fallback_and_preserves_unflushed_execution() {
    for (label, opening, closing) in [("top", "", ""), ("display", ".Bd -literal\n", ".Ed\n")] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{opening}.No A\n.sp bogus\n.No B\n{closing}"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("vertical-space-invalid-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse invalid vertical-space operand");
        let blocks = &document.sections[0].blocks;
        assert_eq!(blocks.len(), 3, "{label}: {blocks:#?}");
        assert!(matches!(
            blocks[0],
            Block::Paragraph { ref children, .. }
                | Block::Preformatted { ref children, .. }
                if inline_text(children) == "A"
        ));
        assert!(matches!(blocks[1], Block::VerticalSpace { lines: 1, .. }));
        assert!(matches!(
            blocks[2],
            Block::Paragraph { ref children, .. }
                | Block::Preformatted { ref children, .. }
                if inline_text(children) == "B"
        ));
        assert!(!visible_document_text(&document).contains("bogus"));
    }

    let nested = parse_manual_bytes(
        std::path::Path::new("vertical-space-invalid-nested.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No A\n.sp bogus\n.No B\n.Ec\n",
    )
    .expect("parse nested invalid vertical-space operand");
    let [Block::Paragraph { children, .. }] = nested.sections[0].blocks.as_slice() else {
        panic!("unexpected nested spacing blocks: {:#?}", nested.sections);
    };
    assert_eq!(inline_text(children), "[A\n\nB", "{children:?}");

    let armed = parse_manual_bytes(
        std::path::Path::new("vertical-space-armed-zero.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \\z\n.sp 1\n.No B C\n",
    )
    .expect("parse vertical space after an armed zero-advance escape");
    let [
        Block::VerticalSpace { lines: 1, .. },
        Block::Paragraph { children, .. },
    ] = armed.sections[0].blocks.as_slice()
    else {
        panic!("unexpected armed spacing blocks: {:#?}", armed.sections);
    };
    assert_eq!(inline_text(children), "BC", "{children:?}");

    for (label, escape, spacing) in [
        ("pending", r"\p", ".sp 1"),
        ("pending-continued", r"\p\c", ".sp 1"),
        ("pending-invalid", r"\p", ".sp bogus"),
        ("pending-continued-invalid", r"\p\c", ".sp bogus"),
        ("armed-then-pending", r"\z\p", ".sp 1"),
        ("armed-pending-continued", r"\z\p\c", ".sp bogus"),
        ("armed-continued-pending", r"\z\c\p", ".sp 1"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {escape}\n{spacing}\n.No B C\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("vertical-space-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse vertical space after a leading word-end break");
        let [
            Block::VerticalSpace { lines: 1, .. },
            Block::VerticalSpace { lines: 1, .. },
            Block::Paragraph { children, .. },
        ] = document.sections[0].blocks.as_slice()
        else {
            panic!(
                "unexpected leading-break spacing blocks: {:#?}",
                document.sections
            );
        };
        assert_eq!(inline_text(children), "B C", "{label}: {children:?}");
    }
}

#[test]
fn pending_inline_execution_is_settled_before_structural_owners() {
    for (label, escape, expects_gap) in [("word-end", r"\p", true), ("zero-advance", r"\z", false)]
    {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {escape}\n.Bl -bullet\n.It\nITEM\n.El\n.No AFTER LAST\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("execution-before-list-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse pending execution before a structural list");
        assert_eq!(
            visible_document_text(&document)
                .split_whitespace()
                .collect::<Vec<_>>(),
            ["DESCRIPTION", "ITEM", "AFTER", "LAST"],
            "{label}: {:#?}",
            document.sections
        );
        let blocks = &document.sections[0].blocks;
        let list_index = blocks
            .iter()
            .position(|block| matches!(block, Block::List { .. }))
            .expect("retain the structural list");
        let after_index = blocks
            .iter()
            .position(|block| {
                matches!(block, Block::Paragraph { children, .. } if inline_text(children) == "AFTER LAST")
            })
            .expect("retain the paragraph after the list");
        assert_eq!(after_index, list_index + 1, "{label}: {blocks:#?}");
        assert_eq!(
            blocks[..list_index]
                .iter()
                .filter(|block| matches!(block, Block::VerticalSpace { lines: 1, .. }))
                .count(),
            usize::from(expects_gap),
            "{label}: {blocks:#?}"
        );
    }
}

#[test]
fn no_fill_execution_state_crosses_transparent_requests_only() {
    for (label, request) in [("font", ".ft B"), ("paragraph-distance", ".PD 1")] {
        let manual = format!(".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n{request}\nB\n.fi\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-man-no-fill-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse transparent no-fill request fixture");
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("expected one preformatted block: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), "AB", "{label}: {children:?}");
        if label == "font" {
            assert!(children.iter().any(|inline| matches!(
                inline,
                Inline::Strong { children } if inline_text(children) == "B"
            )));
        }
    }

    for (label, request) in [
        ("indent", ".in 4"),
        ("temporary-indent", ".ti 4"),
        ("break", ".br"),
        ("space", ".sp"),
        ("fill", ".fi\n.nf"),
        ("repeated-no-fill", ".nf"),
    ] {
        let manual = format!(".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n{request}\nB\n.fi\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-man-no-fill-{label}-boundary.1")),
            manual.as_bytes(),
        )
        .expect("parse no-fill boundary fixture");
        let text = document.sections[0]
            .blocks
            .iter()
            .map(|block| match block {
                Block::Preformatted { children, .. } | Block::Paragraph { children, .. } => {
                    inline_text(children)
                }
                Block::VerticalSpace { .. } => "\n".to_owned(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("AX"), "{label}: {:#?}", document.sections);
        assert!(text.contains('B'), "{label}: {:#?}", document.sections);
        assert!(!text.contains("AB"), "{label}: {:#?}", document.sections);
    }

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-no-fill-margin-character.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n.mc |\nB\n.fi\n",
    )
    .expect("parse no-break margin-character fixture");
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one preformatted block: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "AX B", "{children:?}");

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-no-fill-margin-pending-cell.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\n\\zX\\c\n.mc |\nB\n.fi\n",
    )
    .expect("parse no-break margin pending-cell fixture");
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one preformatted block: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "X B", "{children:?}");

    for (label, first, expected) in [
        ("plain", "A", "A B"),
        ("continued-zero-advance", r"A\zX\c", "AX B"),
    ] {
        let manual = format!(".TH PROBE 1\n.SH DESCRIPTION\n{first}\n.mc |\nB\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-man-filled-margin-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse filled no-break margin-character fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("expected one paragraph: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn invisible_continued_no_fill_cells_reach_temporary_and_no_break_flushes() {
    for (label, header, opening, request, closing, expected) in [
        (
            "mdoc-ti",
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Bd -literal",
            ".ti 4n",
            ".Ed",
            "\nB",
        ),
        (
            "mdoc-mc",
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Bd -literal",
            ".mc |",
            ".Ed",
            " B",
        ),
        (
            "man-ti",
            ".TH PROBE 1\n.SH DESCRIPTION",
            ".nf",
            ".ti 4n",
            ".fi",
            "\nB",
        ),
        (
            "man-mc",
            ".TH PROBE 1\n.SH DESCRIPTION",
            ".nf",
            ".mc |",
            ".fi",
            " B",
        ),
    ] {
        let manual = format!("{header}\n{opening}\n\\&\\c\n{request}\nB\n{closing}\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("invisible-no-fill-cell-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse invisible continued no-fill cell");
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn no_fill_exit_and_document_end_settle_continued_zero_advance_state() {
    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-zero-advance-before-fi.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c\n.fi\nB\n",
    )
    .expect("parse no-fill exit fixture");
    let [
        Block::Preformatted {
            children: literal, ..
        },
        Block::Paragraph {
            children: filled, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!(
            "expected literal and filled blocks: {:#?}",
            document.sections
        );
    };
    assert_eq!(inline_text(literal), "AX", "{literal:?}");
    assert_eq!(inline_text(filled), "B", "{filled:?}");

    let document = parse_manual_bytes(
        std::path::Path::new("inline-man-zero-advance-at-eof.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\zX\\c",
    )
    .expect("parse no-fill EOF fixture");
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one preformatted block: {:#?}", document.sections);
    };
    assert_eq!(inline_text(children), "AX", "{children:?}");
}

#[test]
fn hang_marker_before_any_graph_wipes_the_whole_field() {
    // The exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. A \p whose
    // pass accepted no graph leaves term_fill() resumed at the NEXT word's
    // leading blank (term.c:293-295 with 143-146): that blank rejects the
    // pass before the word's own glyphs, so the entire field - including
    // the X that follows a second marker in the same operand - is
    // unprinted buffer and only BODY survives on the open HANG row.
    let item = definition_item_from_source(
        ".Bl -hang -width 4n\n.It Xo\n.No \"\\p\"\n.No \"X\\p \\p Y\"\n.Xc\n.No BODY\n.El\n",
    );
    assert!(
        item.terms.iter().all(|term| inline_text(term).is_empty()),
        "no part of the rejected field prints: {item:#?}"
    );
    let [Block::Paragraph { children, .. }] = &item.description[..] else {
        panic!("expected the body on the open HANG row: {item:#?}");
    };
    assert_eq!(inline_text(children), "BODY", "{item:#?}");
}

#[test]
fn empty_operand_ends_the_continued_source_row() {
    // The exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. The HEAD's
    // trailing \c set TERMP_NONEWLINE, but the BODY's empty `.No ""`
    // operand still runs term_word(""): its head clears the latch
    // (term.c:588), so the following no-fill NODE_LINE starts a fresh
    // physical row (mdoc_term.c:315-317) and BODY does not run in. A
    // zero-row `.sp 0` in the same position keeps the latch
    // (roff_term.c::pre_sp runs only pre_br()) and the BODY joins.
    let emptied = definition_item_from_source(
        ".nf\n.Bl -hang -width 4n\n.It Xo\n.No X\\c\n.Xc\n.No \"\"\n.No BODY\n.El\n",
    );
    assert!(
        !emptied.layout.inline_term(),
        "the cleared latch ends the row: {emptied:#?}"
    );
    let zero_row = definition_item_from_source(
        ".nf\n.Bl -hang -width 4n\n.It Xo\n.No X\\c\n.sp 0\n.Xc\n.No BODY\n.El\n",
    );
    assert!(
        zero_row.layout.inline_term(),
        "term_newln() alone keeps TERMP_NONEWLINE: {zero_row:#?}"
    );
}

#[test]
fn zero_advance_retreat_keeps_the_graph_after_the_marker() {
    // The exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. The `\z`
    // glyph's TERMP_BACKBEFORE retreat eats the blank directly before the
    // NEXT glyph (term.c:901-908), so the `\p` pass that follows never sees
    // a blank under its marker: X prints on the first row, Y survives on
    // the open HANG row with BODY (term.c:263-367 pass two accepts Y).
    let item = definition_item_from_source(
        ".Bl -hang -width 4n\n.It Xo\n.No \"\\zX\\p\"\n.No \"\\p Y\"\n.Xc\n.No BODY\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "X\nY", "{item:#?}");
    assert!(
        item.description.iter().all(|block| match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                !inline_text(children).starts_with('Y')
            }
            _ => true,
        }),
        "Y owns its term row, not the body: {item:#?}"
    );
}

#[test]
fn empty_operand_blank_after_retreat_wipes_the_remainder() {
    // The exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. The empty
    // operand's term_word("") blank survives the retreat (only Y's own
    // separator blank is eaten), leaving a blank under the armed marker:
    // the next pass rejects with nbr=0 and the unprinted remainder dies
    // (term.c:143-146 with 233-237). X's accepted pass already ended its
    // row (term.c:220); BODY starts the next one.
    let item = definition_item_from_source(
        ".Bl -hang -width 4n\n.It Xo\n.No \"\\zX\\p\"\n.No \"\\p\"\n.No \"\"\n.No Y\n.Xc\n.No BODY\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "X", "{item:#?}");
    assert!(
        item.description.iter().all(|block| match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                !inline_text(children).starts_with('Y')
            }
            _ => true,
        }),
        "the wiped remainder never prints: {item:#?}"
    );
}
