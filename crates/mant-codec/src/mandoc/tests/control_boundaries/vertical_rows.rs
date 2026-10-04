use super::*;

// The selected UTF-8 formatter preserves CVS's executed glyphs: mdoc Ao/Ac
// select the Unicode angle glyphs, while man post_UR always calls term_word
// with literal ASCII "<"/">" (man_term.c:896-908). These assertions were
// rechecked against pristine ASCII/UTF-8/tree/lint before synchronizing them.

#[test]
fn definition_body_uses_continuation_after_generated_words_and_zero_row_requests() {
    // Each exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. In
    // mdoc_term.c::termp_it_pre(), inset BODY's term_word("\\ ") consumes a
    // pending \c before NODE_LINE. In roff_term.c::pre_sp/pre_ce, a zero-row
    // request calls pre_br()/term_newln() without clearing TERMP_NONEWLINE;
    // term.c::term_flushln() can keep a HANG device row open.
    let inset =
        definition_item_from_source(".nf\n.Bl -inset\n.It Xo\n.No X\\c\n.Xc\n.No BODY\n.El\n.fi\n");
    assert_eq!(inline_text(&inset.terms[0]), "X");
    assert!(!inset.inline_term(), "{inset:#?}");
    for request in [".sp 0", ".ce 0", ".rj 0"] {
        let item = definition_item_from_source(&format!(
            ".nf\n.Bl -hang -width 4n\n.It Xo\n.No X\\c\n{request}\n.Xc\n.No BODY\n.El\n.fi\n"
        ));
        assert!(item.inline_term(), "{request}: {item:#?}");
        assert!(item.inline_description().is_some(), "{request}: {item:#?}");
    }
}

#[test]
fn completed_empty_text_rows_survive_filled_output_drains() {
    // Exact combinations checked with fixed CVS -Tascii/-Tlint.
    // man_term.c::print_man_node() sends an empty TEXT to term_vspace();
    // term.c::term_vspace() emits one row independently of later PP or fi/nf.
    // pre_alternate() instead sends BR's empty operands through term_word().
    for (empty, completed_rows) in [
        (".B \"\"", 1),
        (".I \"\"", 1),
        (".B \"\"\n.B \"\"", 2),
        (".B \"\"\n.B \"\" \"\"", 2),
        (".B \"\"\n.B \"\" \"\"\n.B \"\" \"\"", 2),
        (".B \"\"\n.B \"\" \"\"\n.B \"\"", 3),
        (".B \"\"\n\\&\n.B \"\"", 3),
        (".B \"\"\n.BR \"\" \"\"", 1),
        (".B \"\"\n\\&", 2),
    ] {
        for boundary in [".PP", ".fi", ".nf"] {
            let source = format!(
                ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\nBEFORE\n{empty}\n{boundary}\nAFTER\n"
            );
            let document = parse_manual_bytes(
                std::path::Path::new("filled-empty-text-rows.1"),
                source.as_bytes(),
            )
            .unwrap();
            let blocks = &document.sections[0].blocks;
            assert!(
                matches!(&blocks[0], Block::Paragraph { children, .. } if inline_text(children) == "BEFORE"),
                "{empty}, {boundary}: {blocks:#?}"
            );
            assert!(
                matches!(&blocks[1], Block::VerticalSpace { lines, .. } if *lines == completed_rows),
                "{empty}, {boundary}: {blocks:#?}"
            );
            assert!(
                matches!(&blocks[2], Block::Paragraph { children, .. } | Block::Preformatted { children, .. } if inline_text(children) == "AFTER"),
                "{empty}, {boundary}: {blocks:#?}"
            );
            let expected_paragraph_gap = u16::from(boundary == ".PP");
            assert_eq!(
                mant_ir::geometry::block_gap(&blocks[2]),
                expected_paragraph_gap,
                "{empty}, {boundary}: {blocks:#?}"
            );
        }
    }
}

#[test]
fn completed_vertical_rows_do_not_replay_their_invisible_cell() {
    // Exact inputs checked with fixed CVS -Tascii/-Tlint. term.c::term_vspace()
    // completes the empty TEXT row; a subsequent \& occupies just one more
    // row, which term_newln() closes at the next mode or paragraph request.
    for (before, empty_words, request, expected_lines) in [
        ("", ".B \"\"\n", ".nf", 2),
        ("BEFORE\n", ".B \"\"\n", ".nf", 2),
        ("", ".B \"\"\n", ".PP", 2),
        ("BEFORE\n", ".B \"\"\n", ".PP", 2),
        ("", ".B \"\"\n", ".sp 0", 2),
        ("BEFORE\n", ".B \"\"\n", ".sp 0", 2),
        ("", ".B \"\"\n", ".fi", 2),
        ("", ".B \"\"\n.B \"\"\n", ".nf", 3),
    ] {
        let source = format!(
            ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n{before}{empty_words}\\&\n{request}\nAFTER\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("completed-invisible-cell.1"),
            source.as_bytes(),
        )
        .unwrap();
        let blocks = &document.sections[0].blocks;
        let rows = blocks
            .iter()
            .filter_map(|block| match block {
                Block::VerticalSpace { lines, .. } => Some(*lines),
                _ => None,
            })
            .sum::<u16>();
        assert_eq!(rows, expected_lines, "{request}: {blocks:#?}");
        assert_eq!(
            blocks
                .iter()
                .filter(|block| matches!(block, Block::VerticalSpace { lines, .. } if *lines == expected_lines))
                .count(),
            1,
            "{request}: {blocks:#?}"
        );
    }
}

#[test]
fn consecutive_empty_formatter_words_occupy_one_native_row() {
    // Exact no-fill pairs and single/Ns counters checked with fixed CVS
    // -Tascii/-Tlint. term.c::term_word() writes the automatic separator
    // before decoding the second empty word; Ns suppresses that write.
    for words in [
        ".No \"\" No \"\"",
        ".No \\fB No \\fI",
        ".No \"\" Em \"\"",
        ".An \"\" An \"\"",
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n{words}\n.No AFTER\n"
        );
        let document =
            parse_manual_bytes(std::path::Path::new("empty-word-cell.1"), source.as_bytes())
                .unwrap();
        assert!(
            matches!(document.sections[1].blocks.as_slice(), [Block::Preformatted { children, .. }] if inline_text(children) == "\nAFTER"),
            "{words}: {document:#?}"
        );
    }
    for words in [".No \"\"", ".No \"\" Ns No \"\""] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n{words}\n.No AFTER\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("empty-word-counter.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert!(
            matches!(document.sections[1].blocks.as_slice(), [Block::Preformatted { children, .. }] if inline_text(children) == "AFTER"),
            "{words}: {document:#?}"
        );
    }
}

#[test]
fn empty_word_cell_survives_filled_control_boundaries() {
    // Exact br/sp/Pp forms checked with fixed CVS -Tascii/-Tlint. A native
    // separator cell is completed before the following line/space request.
    for (request, expected_rows) in [(".br", 1), (".sp 0", 1), (".sp 1", 2), (".Pp", 2)] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.No \"\" No \"\"\n{request}\n.No AFTER\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("empty-word-filled-boundary.1"),
            source.as_bytes(),
        )
        .unwrap();
        let blocks = &document.sections[1].blocks;
        let vertical_rows: u16 = blocks
            .iter()
            .filter_map(|block| match block {
                Block::VerticalSpace { lines, .. } => Some(*lines),
                _ => None,
            })
            .sum();
        let hard_rows = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph { children, .. } => Some(
                    children
                        .iter()
                        .filter(|inline| matches!(inline, Inline::LineBreak { .. }))
                        .count(),
                ),
                _ => None,
            })
            .sum::<usize>();
        assert_eq!(
            vertical_rows + u16::try_from(hard_rows).unwrap(),
            expected_rows,
            "{request}: {blocks:#?}"
        );
    }
}
