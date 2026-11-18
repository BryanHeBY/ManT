//! The written tbl slots affect width without exposing device decorations.

use super::{Block, assert_prefix, between, first_table, input, lower, projections, whole_word};

#[test]
fn overstrike_then_motion_preserves_the_native_first_pass_word_boundary() {
    // Exact pinned ASCII and UTF-8 agree on these three adjacent widths.
    // term_word() retains every encode1 slot for absolute motion, while
    // term_fill() reduces the visual width after each backspace.
    for (operand, projected, absolute_center, absolute_right) in [
        ("b", "XYZb", 12, 11),
        (r"\o'ab'", "XYZb", 10, 9),
        (r"\o'aab'", "XYZb", 10, 9),
        (r"\fB\o'ab'", "XYZb", 10, 9),
        (r"\fI\o'aab'", "XYZb", 10, 9),
        (r"\o'a b'", "XYZb", 10, 9),
        (r"\o'a '", "XYZa", 12, 11),
    ] {
        for (motion, center, right) in [
            (r"\h'|6n'", absolute_center, absolute_right),
            (r"\h'1n'", 11, 10),
        ] {
            for (layout, threshold) in [("l", 12), ("c", center), ("r", right)] {
                for width in 9..=11 {
                    let source = input(
                        "head",
                        ".No LONGPREFIX\n.br\n",
                        layout,
                        &format!("XYZ{operand}{motion}\n{}", "W".repeat(width)),
                    );
                    let query = lower(&source);
                    let markdown = mant_codec::encode::render_markdown(&query);
                    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
                    let separated = width >= threshold;
                    for contents in [&query, &readback] {
                        let text = mant_render::render_query_text(contents);
                        let joined = format!("LONGPREFIX{projected}");
                        let split = format!("LONGPREFIX {projected}");
                        assert!(
                            text.contains(if separated { &split } else { &joined }),
                            "{source}\n{text}"
                        );
                        assert_eq!(text.matches(projected).count(), 1, "{source}\n{text}");
                        for (word, expected) in [
                            ("LONGPREFIX", separated),
                            (projected, separated),
                            (&joined, !separated),
                        ] {
                            assert_eq!(
                                whole_word(contents, word).matches.len(),
                                usize::from(expected),
                                "{source}\n{text}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn an_unprojected_prior_cell_separates_only_when_its_native_pass_prints_ink() {
    // term_field prints accepted drawing strokes but defers the ordinary
    // or fixed blanks. Columns with a real rule independently separate.
    for (earlier, ink) in [
        ("", false),
        (r"\h'2n'", false),
        (r"\l'2n '", false),
        (r"\l'2n.'", true),
        (r"\l'2n'", true),
        ("_", true),
    ] {
        for layout in ["l0 l", "l1|l", "l l"] {
            assert_prefix(
                layout,
                &format!("{earlier}\tXYZ"),
                "1\tQ",
                ".sp -1",
                ink || layout == "l1|l",
            );
        }
    }

    // A non-positive length takes the actual column width. The first
    // column remains narrow even when a following row contributes width.
    for (drawing, ink) in [
        (r"\l'0n.'", true),
        (r"\l'-1n.'", true),
        (r"\l'0n'", true),
        (r"\l'0n '", false),
    ] {
        for next in ["", "12"] {
            assert_prefix(
                "l0 l",
                &format!("{drawing}\tXYZ"),
                &format!("{next}\tQ"),
                ".sp -1",
                ink,
            );
        }
    }
}

#[test]
fn a_drawn_graph_commits_its_row_without_absorbing_the_next_word_or_owner() {
    for owner in ["head", "body", "block"] {
        for (middle, rows) in [
            (".", &[".", "XYZ"][..]),
            // The native UTF-8 device preserves U+00A0 on this row.
            (r"\~", &["\u{a0}", "XYZ"]),
            (r"\l'2n.'", &["", "XYZ"]),
            (r"\l'2n'", &["", "XYZ"]),
            (r"\l'2n '", &[""]),
            (r"\l'0n.'", &["", "XYZ"]),
            (r"\l'-1n.'", &["", "XYZ"]),
            (r"\l'0n '", &[""]),
            ("", &[""]),
        ] {
            let source = input(owner, "", "l", &format!("START\\p {middle}\\p XYZ\nLAST"));
            let query = lower(&source);
            for text in projections(&query) {
                assert_eq!(between(&text, "START", "LAST"), rows, "{source}\n{text}");
                for word in ["START", "LAST", "ENDING"] {
                    assert_eq!(text.matches(word).count(), 1, "{source}\n{text}");
                }
            }
            assert_eq!(
                whole_word(&query, "XYZ").matches.len(),
                usize::from(rows.contains(&"XYZ")),
                "{source}"
            );

            let blocks = &query.document.as_ref().unwrap().sections[1].blocks;
            if owner == "head" {
                let [Block::DefinitionList { items, .. }] = blocks.as_slice() else {
                    panic!("{source}: expected the HEAD owner");
                };
                let term = mant_ir::inline_plain_text(&items[0].terms[0]);
                assert!(term.contains("START") && term.contains("LAST"), "{source}");
                assert_eq!(term.contains("XYZ"), rows.contains(&"XYZ"), "{source}");
                assert!(!term.contains("ENDING"), "{source}");
            } else {
                let Some(Block::Table { rows: table, .. }) = first_table(blocks) else {
                    panic!("{source}: expected the data cell owner");
                };
                let [Block::Paragraph { children, .. }, ..] = table[0].cells[0].blocks.as_slice()
                else {
                    panic!("{source}: expected the first cell content");
                };
                let cell = mant_ir::inline_plain_text(children);
                assert!(cell.contains("START"), "{source}");
                assert_eq!(cell.contains("XYZ"), rows.contains(&"XYZ"), "{source}");
                assert!(
                    !cell.contains("LAST") && !cell.contains("ENDING"),
                    "{source}"
                );
            }
        }
    }
}
