//! Horizontal motion, drawings and live word edits at the CLI boundary.

use super::{
    EDITABLE_FIELD, FIELD_PROBE, LinkTarget, assert_readback_modes, assert_readbacks, description,
    document_links, render, search_visible_word, search_visible_word_with_format, without_sgr,
};

#[test]
fn leading_horizontal_motion_changes_native_acceptance_without_replaying_padding() {
    // Each source was run through the fixed CVS. term.c:678–714 makes
    // HORIZ clear or rewind BACKBEFORE before term_fill() accepts cells.
    let heading = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    for (first, second, expected) in [
        ("ALPHA", r"\h'-1n'BETA", "ALPHABETA\nOMEGA"),
        (
            r"ALPHA\p\:\zX",
            r"\h'-1n'\&\p\: \p BETA",
            "ALPHA\nX\n\nOMEGA",
        ),
        (
            r"ALPHA\zX\p\: ",
            r"\h'-1n'\&\p\: \p BETA",
            "ALPHAX\n\n\nOMEGA",
        ),
        (r"ALPHA\zX", r"\h'1n'\z \p BETA", "ALPHAX\n\nOMEGA"),
    ] {
        let source =
            format!("{heading}.No \"{first}\"\n.No \"{second}\"\n.No \"\"\n.br\n.No OMEGA\n");
        assert_readbacks(&source, expected, None);
    }
}

#[test]
fn negative_word_entry_preserves_each_surviving_cell_and_owner() {
    // These exact inputs were run with the pinned CVS. term.c:701–714
    // rewinds without shortening lastcol; an SP advances across the old
    // cells without overwriting them (term.c:948–952). Markers can replace
    // the tab reference or the live BACKBEFORE glyph before term_fill().
    let heading = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    for (word, expected, label) in [
        (r#".No "\h'-1n'   BETA""#, "ALPHAX  BETA\nOMEGA", None),
        (r#".No "\h'-2n'   BETA""#, "ALPHAX BETA\nOMEGA", None),
        (r#".No "\h'-1n'    BETA""#, "ALPHAX   BETA\nOMEGA", None),
        (r#".No "\h'-2n' BETA""#, "ALPHAXBETA\nOMEGA", None),
        (r#".No "\h'-1n'\p BETA""#, "ALPHAX\nBETA\nOMEGA", None),
        (r#".No "\h'-2n'\p BETA""#, "ALPHABETA\nOMEGA", None),
        (r#".No "\h'-2n'\: BETA""#, "ALPHABETA\nOMEGA", None),
        (
            r#".No "\h'-1n'\p\:    BETA""#,
            "ALPHAX\n    BETA\nOMEGA",
            None,
        ),
        (r#".No "\h'-24u'\p BETA""#, "ALPHAX\nBETA\nOMEGA", None),
        (r#".No "\h'-25u'\p BETA""#, "ALPHAX\nBETA\nOMEGA", None),
        (r#".No "\h'-47u'\p BETA""#, "ALPHAX\nBETA\nOMEGA", None),
        (r#".No "\h'-48u'\p\: BETA""#, "ALPHA\n BETA\nOMEGA", None),
        (r#".Em "\h'-2n'\p\: BETA""#, "ALPHA\n BETA\nOMEGA", None),
        (
            r#".Lk https://x.io "\h'-2n'\p\: BETA""#,
            "ALPHA\n BETA: https://x.io\nOMEGA",
            Some("BETA"),
        ),
        // Zero distance does not take the negative HORIZ branch. The
        // unchanged marker position rejects BETA in the actual pass.
        (r#".No "\h'0n'\p\: BETA""#, "ALPHAX\n\nOMEGA", None),
    ] {
        let source = format!("{heading}.No \"ALPHA\\zX\"\n{word}\n.No \"\"\n.br\n.No OMEGA\n");
        assert_readbacks(&source, expected, label);
    }
}

#[test]
fn a_proved_word_entry_rewinds_ordinary_glyphs_before_accepting_the_break() {
    // term.c::term_word() rewinds the live field through ordinary A and
    // writes the marker before term_fill(). Both fixed devices accept
    // ALPH and BETA on separate rows before the following OMEGA.
    let source = r#".Dd November 10, 2026
.Dt CHECK 1
.Os
.Sh NAME
.Nm check
.Nd word audit
.Sh DESCRIPTION
.No "ALPHA\zX"
.No "\h'-3n'\p\: BETA"
.No ""
.br
.No OMEGA
"#;
    assert_readbacks(source, "ALPH\nBETA\nOMEGA", None);
}

#[test]
fn same_operand_motion_uses_the_written_slots_before_its_marker() {
    // CVS ASCII and UTF-8 agree. term.c:678–714 evaluates both forms of HORIZ
    // from col; term_fill() receives the marker after that retreat.
    let heading = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    for (distance, expected) in [
        ("-1n", "ALPHA\n BETA\nOMEGA"),
        ("-2n", "ALPH\n BETA\nOMEGA"),
        ("0n", "ALPHAX\n\nOMEGA"),
        ("1n", "ALPHAX\n\nOMEGA"),
        ("|6n", "ALPHA\n BETA\nOMEGA"),
    ] {
        let inline =
            format!("{heading}.No \"ALPHA\\zX \\h'{distance}'\\p\\: BETA\"\n.br\n.No OMEGA\n");
        assert_readbacks(&inline, expected, None);
    }
}

#[test]
fn drawing_word_uses_the_native_field_lifetime() {
    let heading = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    let source = format!(
        "{heading}.No \"ALPHA\\zX\"\n.br\n.No \"P\\l'1n '\\h'0n'\\h'|5n'\\p NEEDLE\"\n.No \"\"\n.br\n.No OMEGA\n"
    );
    // Both fixed CVS devices output these four rows. br retires the old
    // field, so the following absolute HORIZ starts from a fresh cursor.
    assert_readbacks(&source, "ALPHAX\nP\nNEEDLE\nOMEGA", None);

    // Without br, both devices still print NEEDLE. The motion can address
    // the earlier word, so an incomplete geometry receipt cannot reject it.
    let live = source.replacen("\n.br\n", "\n", 1);
    assert!(
        description(&render(&live, "roff", "text", "never", false)).contains("NEEDLE"),
        "{live}"
    );
}

#[test]
fn local_slot_edits_keep_their_effect_after_a_prior_word() {
    // Pinned term.c:678–714 and 790–796. Here the retreat and overstrike trim
    // affect only bytes of the arriving word, even with an occupied field.
    let heading = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test page\n.Sh DESCRIPTION\n";
    for (word, expected) in [
        (r"Q\p \o' '\p XYZ", "BEFORE Q\nXYZ LAST\nEND"),
        (r"\l'1n.'\h'-1n'\p XYZ", "BEFORE\n\nEND"),
    ] {
        let source = format!("{heading}.No BEFORE\n.No \"{word}\"\n.No LAST\n.br\n.No END\n");
        assert_readbacks(&source, expected, None);
    }
}

#[test]
fn negative_word_entry_keeps_the_exact_prior_scalar_and_backafter_order() {
    // Pinned term.c:678–681 cancels an armed BACKAFTER without moving
    // the cursor. With BACKBEFORE alone, only its final X can be
    // replaced; the identically spelled earlier X keeps its own style.
    let heading = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    for (first, second, expected) in [
        (r"ALPHA\zX", r"\h'-2n'\p\: BETA", "ALPHA\n BETA\nOMEGA"),
        (r"ALPHAX\zX", r"\h'-2n'\p\: BETA", "ALPHAX\n BETA\nOMEGA"),
        (
            r"ALPHA\fBX\fR\zX",
            r"\h'-2n'\p\: BETA",
            "ALPHAX\n BETA\nOMEGA",
        ),
        (r"ALPHA\zX\z", r"\h'-1n'\:\p BETA", "ALPHAX BETA\nOMEGA"),
        (r"ALPHA\z ", r"\h'-47u' BETA", "ALPHA   BETA\nOMEGA"),
    ] {
        let source = format!("{heading}.No \"{first}\"\n.No \"{second}\"\n.br\n.No OMEGA\n");
        assert_readbacks(&source, expected, None);
    }
}

#[test]
fn negative_word_entry_places_crossed_spaces_at_their_source_position() {
    // Fixed CVS term.c:948–952 lets an internal source SP cross TABREF
    // (no visible contribution) or the automatic SP (represented once
    // at its new position). encode1() writes style backspaces before the
    // graph, so in the link case it also replaces the old padding.
    let heading = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    for (word, expected, label) in [
        (r#".No "\h'-1n'B ETA""#, "ALPHAXB ETA\nOMEGA", None),
        (r#".No "\h'-2n'B ETA""#, "ALPHABETA\nOMEGA", None),
        (r#".Em "\h'-2n'BE TA""#, "ALPHABE TA\nOMEGA", None),
        (r#".No "\h'-2n'B  ETA""#, "ALPHAB ETA\nOMEGA", None),
        (r#".No "\h'-2n'B   ETA""#, "ALPHAB  ETA\nOMEGA", None),
        (
            r#".Lk https://x.io "\h'-1n'B ETA""#,
            "ALPHAXB ETA: https://x.io\nOMEGA",
            Some("B ETA"),
        ),
        (
            r#".Lk https://x.io "\h'-2n'B ETA""#,
            "ALPHAB ETA: https://x.io\nOMEGA",
            Some("B ETA"),
        ),
        (r#".No "\h'-2n'\p\zY  BETA""#, "ALPHAY\nBETA\nOMEGA", None),
    ] {
        let source = format!("{heading}.No \"ALPHA\\zX\"\n{word}\n.br\n.No OMEGA\n");
        assert_readbacks(&source, expected, label);
    }
}

#[test]
fn interword_spacing_uses_the_last_accepted_cell_before_a_pending_glyph() {
    // CVS writes an ordinary SP before \&, then an empty overstrike
    // either trims an extra separator or leaves the old SP for T to
    // overstrike. Observe both directions across native font scopes.
    for (word, expected, search, count) in [
        (r#".Sy A No "\&" No "\z\o''T""#, "A T", "AT", 0),
        (r#".No A No "\&" No "\z\o''T""#, "A T", "AT", 0),
        (r#".Ic abc No "\&" Ns No "\z\o''T""#, "abc T", "abcT", 0),
        (r#".Fl abc No "\:" No "\z\o''T""#, "-abc T", "abcT", 0),
        (r#".No "P\zQ" No "\&" Ns No "\z\o''T""#, "PQT", "PQT", 1),
        (r#".No "P\zQ" No "\:" Ns No "\o''T""#, "PQT", "PQT", 1),
    ] {
        let source = format!("{FIELD_PROBE}STARTFLAG\n.br\n{word}\n.br\nENDFLAG\n");
        let expected_rendering = format!("STARTFLAG\n{expected}\nENDFLAG");
        let plain = render(&source, "roff", "text", "never", false);
        assert_eq!(description(&plain), expected_rendering, "{source}");
        let ansi = render(&source, "roff", "text", "always", false);
        assert_eq!(without_sgr(&ansi), plain, "{source}");
        let markdown = render(&source, "roff", "markdown", "never", false);
        let roundtrip = render(&markdown, "markdown", "text", "never", false);
        assert_eq!(description(&roundtrip), expected_rendering, "{source}");
        assert_eq!(search_visible_word(&source, search), count, "{source}");
    }
}

#[test]
fn relative_word_rewind_needs_no_overstrike_or_line_break_escape() {
    // term_word's relative HORIZ retreats one native slot. T overwrites
    // M both in one operand and across an independently executed No.
    for (word, expected) in [
        (r"PM\h'-1n'T", "PT"),
        (r#".No PM Ns No "\h'-1n'T""#, "PT"),
        ("PMT", "PMT"),
    ] {
        let source = format!("{FIELD_PROBE}STARTFLAG\n.br\n{word}\n.br\nENDFLAG\n");
        assert_readbacks(&source, &format!("STARTFLAG\n{expected}\nENDFLAG"), None);
        assert_eq!(
            search_visible_word(&source, "PT"),
            u64::from(expected == "PT")
        );
    }
}

#[test]
fn accepted_cells_keep_their_glyphs_across_replayed_separators() {
    // CVS term.c::term_flushln and term_field consume gaps separately
    // from visible glyphs. The last motion-only input changes col, not
    // lastcol, so it cannot erase a previously buffered row end.
    for (word, reading, hit, miss, labels) in [
        (
            r#".No "\zQ\p" Ns No A No "\pT" Ns No "\h'-2n'Y""#,
            "A\nYT",
            "YT",
            "Q",
            &[][..],
        ),
        (
            r#".No "\X'tty: link https://one.test'P\X'tty: link' \pQ" No "\h'2n'\p B""#,
            "P Q\n\nB",
            "B",
            "QB",
            &["P"][..],
        ),
        (
            r#".No "\X'tty: link https://one.test'P\X'tty: link'\p Q" No "\p " No "\h'-1n'""#,
            "P\nQ",
            "Q",
            "P Q",
            &["P"][..],
        ),
    ] {
        let source = format!("{EDITABLE_FIELD}{word}\n");
        assert_readbacks(&source, reading, None);
        for (input, format) in [
            (source.clone(), "roff"),
            (
                render(&source, "roff", "markdown", "never", false),
                "markdown",
            ),
        ] {
            let found: Vec<_> = document_links(&input, format)
                .into_iter()
                .map(|(target, label)| {
                    assert_eq!(
                        target,
                        LinkTarget::External {
                            uri: "https://one.test".into()
                        }
                    );
                    label
                })
                .collect();
            assert_eq!(found, labels, "{source}");
            assert_eq!(
                search_visible_word_with_format(&input, format, hit),
                1,
                "{source}"
            );
            assert_eq!(
                search_visible_word_with_format(&input, format, miss),
                0,
                "{source}"
            );
        }
    }
}

#[test]
fn adjacent_markers_leave_the_live_field_editable_across_word_boundaries() {
    // term_word writes both markers into one field before term_fill
    // accepts it. The terminal rewind can land in either source word.
    for (join, distance, expected) in [
        ("Ns No", 3, "PTA"),
        ("Ns No", 4, "PTA"),
        ("Ns No", 5, "TA"),
        ("No", 4, "PTA"),
        ("No", 5, "PT A"),
    ] {
        let source =
            format!(r#"{EDITABLE_FIELD}.No "P\p" {join} "\pA" No "\h'-{distance}n'T""#) + "\n";
        assert_readbacks(&source, expected, None);
        if expected != "PT A" {
            assert_eq!(search_visible_word(&source, expected), 1, "{source}");
        }
        assert_eq!(search_visible_word(&source, "PA T"), 0, "{source}");
    }
}

#[test]
fn word_end_markers_preserve_the_cells_and_owners_of_inner_separators() {
    // term_fill treats ASCII_NBRSP as a printable cell and accepts the
    // earlier breakable blank as a row end. A later term_word can replace
    // the linked glyph or the glyph after a horizontal advance.
    let link = r#".No "A\p \X'tty: link https://one.test'B\X'tty: link'" No "#;
    let overwritten = format!("{EDITABLE_FIELD}{link}\"\\h'-3n'T\"\n");
    let retained = format!("{EDITABLE_FIELD}{link}\"T\"\n");
    assert_readbacks(&overwritten, "A\nT", None);
    assert_readbacks(&retained, "A\nB T", None);
    for (source, expected) in [(&overwritten, &[][..]), (&retained, &["B"][..])] {
        let markdown = render(source, "roff", "markdown", "never", false);
        for (text, format) in [(source.as_str(), "roff"), (markdown.as_str(), "markdown")] {
            let labels: Vec<_> = document_links(text, format)
                .into_iter()
                .filter_map(|(target, label)| {
                    assert_eq!(
                        target,
                        LinkTarget::External {
                            uri: "https://one.test".into()
                        }
                    );
                    (!label.is_empty()).then_some(label)
                })
                .collect();
            assert_eq!(labels, expected, "{source}");
        }
    }
    let source = format!(r#"{EDITABLE_FIELD}.No "A\p\h'2n'B" No "\h'-2n'T""#) + "\n";
    assert_readbacks(&source, "A  T", None);
}

#[test]
fn a_later_operand_can_remove_the_blank_that_would_end_the_previous_pass() {
    // term.c::encode1 retracts the last SP before writing the next graph.
    // term_fill only sees a break opportunity if a separator survives.
    for (next, expected) in [
        (r"B\pC", "ABC"),
        (r"\zBC", "AC"),
        (r"\zB C", "ABC"),
        (r"B\p\&C", "ABC"),
        (r"\zB\&C", "AC"),
        (r"\zB\:C", "AB\nC"),
        (r"\zB\:\zC", "AB\nC"),
        (r"\zB\p\:C", "AB\nC"),
        (r"\o#BB#\p\:C", "AB\nC"),
    ] {
        let source = format!(r#"{EDITABLE_FIELD}.No "\zA\p " Ns No "{next}""#) + "\n";
        assert_readbacks(&source, expected, None);
        assert_eq!(
            search_visible_word(&source, "AB"),
            u64::from(expected.starts_with("AB\n")),
            "{source}"
        );
    }
}

#[test]
fn long_device_advances_leave_complete_words_editable_after_them() {
    // The pinned term_word() advances col independently of its later
    // term_fill()/term_field() view. These short rewinds replace B or C.
    for length in [5, 63, 64, 65, 96, 128] {
        for gap in [
            format!(r"\h'{length}n'"),
            format!(r"\h'|{}n'", length + 1),
            format!(r"\h'1n'\h'{}n'", length - 1),
            format!(r"\l'{length}n '"),
        ] {
            for (after, wanted, erased) in [("", "AX", 'B'), (r#" Ns No "DC""#, "ABDX", 'C')] {
                let source =
                    format!(r#"{EDITABLE_FIELD}.No "A{gap}B"{after} No "\h'-2n'X""#) + "\n";
                let markdown = render(&source, "roff", "markdown", "never", false);
                for text in [
                    render(&source, "roff", "text", "never", false),
                    render(&markdown, "markdown", "text", "never", false),
                ] {
                    let body = description(&text);
                    assert_eq!(
                        body.chars()
                            .filter(char::is_ascii_uppercase)
                            .collect::<String>(),
                        wanted,
                        "{source}"
                    );
                    assert!(!body.contains(erased), "{source}");
                }
            }
        }
    }
}

#[test]
fn active_words_keep_raw_glyphs_and_advances_through_later_edits() {
    // Pinned term.c::term_word/encode1 keeps the word-end marker, spaces,
    // and overstrike bytes editable until term_flushln() accepts a pass.
    for (word, expected, hit, miss, label) in [
        (r#".No "P \zMA" No "\h'-3n'T""#, "P MTA", "MTA", "TA", None),
        (
            r#".No P No "\zMA" No "\h'-3n'T""#,
            "P MTA",
            "MTA",
            "TA",
            None,
        ),
        (r#".Sy "A\pB" No "\h'-2n'T""#, "AT", "AT", "AB T", None),
        (r#".No "A\h'2n'B" No "\h'-2n'T""#, "A  T", "T", "B", None),
        (r#".No "A\l'1n 'B" No "\h'-2n'T""#, "A T", "T", "B", None),
        (
            r#".Sy "P\h'2n'\X'tty: link https://x.io'M\X'tty: link'AB" No "\h'-2n'T""#,
            "P  MAT",
            "MAT",
            "MAB",
            Some("M"),
        ),
        (
            r#".Sy "P\l'1n '\X'tty: link https://x.io'M\X'tty: link'AB" No "\h'-2n'T""#,
            "P MAT",
            "MAT",
            "MAB",
            Some("M"),
        ),
        (
            r#".Sy "P\X'tty: link https://x.io'M\pA\X'tty: link'B" No "T" Ns No "\h'-3n'Y""#,
            "PMAY\nT",
            "PMAY",
            "PMAB",
            Some("MA"),
        ),
    ] {
        let source = format!("{FIELD_PROBE}{word}\n");
        // Addressable Markdown uses raw HTML anchor tags, which are
        // outside the documented semantic reimport subset.
        assert_readback_modes(&source, expected, label, &[false]);
        assert_eq!(search_visible_word(&source, hit), 1, "{source}");
        assert_eq!(search_visible_word(&source, miss), 0, "{source}");
    }
}
