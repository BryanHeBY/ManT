//! Glyph, style and link ownership after native overprinting.

use mant_ir::{Inline, inline_plain_text};

use super::{
    EDITABLE_FIELD, FIELD_PROBE, LinkTarget, assert_link, assert_readbacks, description,
    document_links, render, run_text_input, search_visible_word, search_visible_word_with_format,
};

#[test]
fn later_native_writes_resolve_backlines_without_losing_source_owners() {
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";
    // term_field() skips NBRZW after BS. A following word can print a
    // blank or glyph in the old column; term_newln() alone cannot.
    for overprint in [r"P\o'CD'", r"P\zCD"] {
        for (tail, result) in [
            (" No Y", "P Y"),
            (" Ns No Y", "PY"),
            ("", "PC"),
            ("\n.br\n.No Y", "PC\nY"),
        ] {
            let source = format!("{header}.No \"{overprint}\" Ns No \"\\h'-1n'\\&\"{tail}\n");
            assert_readbacks(&source, result, None);
            assert_eq!(
                search_visible_word(&source, "PC"),
                u64::from(result.starts_with("PC")),
                "{source}"
            );
            if matches!(result, "P Y" | "PY") {
                assert_eq!(search_visible_word(&source, result), 1, "{source}");
            }
        }
    }
    // encode1() writes both author cells and a real BS. Replacing BS
    // reveals M, while replacing A leaves M's overstrike active.
    for (word, retreat, result, label) in [
        (r"\zMA", 2, "T", None),
        (r"\zMA", 3, "MTA", None),
        (
            r"\zM\X'tty: link https://x.io'A\X'tty: link'",
            4,
            "MTA",
            Some("A"),
        ),
    ] {
        let source = format!("{header}.No \"{word}\" No \"\\h'-{retreat}n'T\"\n");
        assert_readbacks(&source, result, label);
        assert_eq!(search_visible_word(&source, result), 1, "{source}");
    }
    // The two native bold paints are one linked author, even when the
    // neighboring invisible link cells are included in the raw replay.
    let source = format!(
        "{header}.Sy \"\\X'tty: link https://x.io'M\\X'tty: link'\" No \"\\h'-1n'T\" No \"\\h'-2n'Y\"\n"
    );
    assert_readbacks(&source, "MY", Some("M"));
    let ansi = render(&source, "roff", "text", "always", false);
    assert!(
        ansi.lines().last().unwrap().starts_with("\u{1b}[1m"),
        "{ansi:?}"
    );
    assert_eq!(search_visible_word(&source, "MY"), 1);
}

#[test]
fn replay_retires_or_moves_the_exact_gap_and_retains_surviving_bold_copies() {
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";
    // term_word() traverses preserved cells, but term_field() prints a
    // surviving advance where it actually lands, including after a rewind.
    for (word, expected) in [
        (r#".No PMAB No "\h'-2n'T" No Y No "\h'-3n'Z""#, "PMATZY"),
        (r#".No PMAB No "\h'-2n'T" No Y No "\h'-5n'Z""#, "PMZT Y"),
        (r#".No PMAB No "\h'-2n'T" No Y Ns No "\h'-3n'Z""#, "PMAZ Y"),
        (r#".No "P MA B" No "\h'-2n'T" No "\h'-3n'Y""#, "P MAYT"),
    ] {
        let source = format!("{header}{word}\n");
        assert_readbacks(&source, expected, None);
        assert_eq!(search_visible_word(&source, expected), 1, "{source}");
    }
    // term.c::encode1() writes both bold copies. When the original
    // projection cell is replaced, its earlier copy can still own ink.
    // In the last case the second rewind removes the backspace between
    // them and both fixed terminal devices print the two Ms separately.
    // The two copies still share one authored link occurrence.
    let word = r#".Sy "P\X'tty: link https://x.io'M\X'tty: link'AB""#;
    for (tail, result, label) in [
        (r#"No "\h'-9n'T" No "\h'-6n'Y""#, "YTAB", ""),
        (r#"Ns No "\h'-7n'T" Ns No "\h'-6n'Y""#, "YMTAB", "M"),
        (r#"Ns No "\h'-9n'T" Ns No "\h'-4n'Y""#, "YMTMAB", "M"),
    ] {
        let source = format!("{header}{word} {tail}\n");
        let plain = render(&source, "roff", "text", "never", false);
        assert_eq!(description(&plain), result, "{source}");
        assert_link(&source, "roff", label);
        // Addressable export writes the implicit Sy tag as raw anchor HTML,
        // which the documented Markdown reimport contract retains as HTML.
        let markdown = render(&source, "roff", "markdown", "never", false);
        assert_eq!(
            description(&render(&markdown, "markdown", "text", "never", false)),
            result,
            "{markdown}"
        );
        assert_link(&markdown, "markdown", label);
        assert_eq!(search_visible_word(&source, result), 1, "{source}");
        if result == "YMTMAB" {
            assert_eq!(search_visible_word(&source, "YMTAB"), 0, "{source}");
        }
    }
}

#[test]
fn combining_overstrike_uses_the_accepted_field_to_keep_or_cover_its_base() {
    let header = ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd word audit\n.Sh DESCRIPTION\n";
    // Fixed CVS UTF-8 writes the base, a backspace and the non-advancing
    // accent (term.c::encode1 and term_field). The portable reading stream
    // keeps their source order. A later cell on the same native pass can
    // cover the base, while a subsequent physical line cannot.
    for (input, expected) in [
        (".No \"ALPHA\\ze\u{301}\"", "ALPHAe\u{301}"),
        (r#".No "ALPHA\ze\[u0301]""#, "ALPHAe\u{301}"),
        (".No \"ALPHA\\ze\u{301}\u{300}\"", "ALPHAe\u{301}\u{300}"),
        (".No \"ALPHA\\ze\u{301}TAIL\"", "ALPHA\u{301}TAIL"),
        (r#".No "ALPHA\ze\[u0301]TAIL""#, "ALPHA\u{301}TAIL"),
        (".No \"ALPHA\\ze\\z\u{301}TAIL\"", "ALPHA\u{301}TAIL"),
        (".No \"ALPHA\\ze\u{301}\" Ns No TAIL", "ALPHA\u{301}TAIL"),
        (
            ".No \"ALPHA\\ze\u{301}\"\n.No \"TAIL\"",
            "ALPHA\u{301} TAIL",
        ),
        (".No \"ALPHA\\ze \u{301}TAIL\"", "ALPHAe\u{301}TAIL"),
        (
            ".No \"ALPHA\\ze\u{301}\"\n.br\n.No TAIL",
            "ALPHAe\u{301}\nTAIL",
        ),
    ] {
        assert_readbacks(&format!("{header}{input}\n"), expected, None);
    }

    let styled = format!("{header}.No \"ALPHA\\fB\\ze\\fI\u{301}\"\n");
    let bundle: mant_protocol::QueryBundle =
        serde_json::from_str(&render(&styled, "roff", "json", "never", false)).unwrap();
    let document = bundle.document.unwrap();
    let mant_ir::Block::Paragraph { children, .. } = &document.sections.last().unwrap().blocks[0]
    else {
        panic!("expected the combining glyph's paragraph");
    };
    let [
        Inline::Text { value },
        Inline::Strong { children: base },
        Inline::Emphasis { children: mark },
    ] = children.as_slice()
    else {
        panic!("expected separate base and mark styles: {children:?}");
    };
    assert_eq!(value, "ALPHA");
    assert_eq!(inline_plain_text(base), "e");
    assert_eq!(inline_plain_text(mark), "\u{301}");
}

#[test]
fn trimming_the_native_field_can_reveal_an_earlier_overstrike_glyph() {
    // term.c::term_word trims raw slots. term_field writes the earlier
    // graph and its backspace even when the graph that covered it is gone.
    let header = ".TH CHECK 1 \"November 19, 2026\"\n.SH NAME\ncheck \\- test\n.SH DESCRIPTION\n";
    for (word, text, label) in [
        (r"HEAD\o'AB'\o' '", "HEADA", None),
        (
            r"HEAD\X'tty: link https://x.io'\o'AB'\o' '\X'tty: link'",
            "HEADA",
            Some("A"),
        ),
        (
            r"HEAD\X'tty: link https://x.io'\o'AB'\X'tty: link'\o' '",
            "HEADB",
            Some("B"),
        ),
        (
            r"HEAD\X'tty: link https://x.io'\o'AB'\o' 'C\X'tty: link'",
            "HEADC",
            Some("C"),
        ),
        (concat!(r"HEAD\o'AB'\o' '\c", "\nZ"), "HEADZ", None),
        (
            concat!(
                r"HEAD\X'tty: link https://x.io'\o'AB'\o' '\X'tty: link'\c",
                "\nZ",
            ),
            "HEADZ",
            None,
        ),
    ] {
        let source = format!("{header}{word}\n.br\nENDTEXT\n");
        assert_readbacks(&source, &format!("{text}\nENDTEXT"), label);
        if label.is_none() {
            assert_eq!(document_links(&source, "roff"), []);
        }
    }
}

#[test]
fn later_operands_can_expose_or_trim_author_slots_from_the_active_field() {
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n.ll 140n\n";
    // In the first case the final operand rewinds over the drawing and
    // leaves PREX followed by a terminal backspace at the end of the pass.
    // In the second case each blank overstrike trims an earlier letter.
    for (head, first) in [
        (
            r#".No "PRE\zX" Ns No "\l'1n.'" Ns No "\h'-1n'\p NEEDLE""#,
            "PREX",
        ),
        (r#".No "PRE" Ns No "\o' '" Ns No "\o' '\p NEEDLE""#, "P"),
    ] {
        let source = format!("{header}{head}\nTAILWORD\n.Pp\nENDTEXT\n");
        assert_readbacks(
            &source,
            &format!("{first}\nNEEDLE TAILWORD\n\nENDTEXT"),
            None,
        );
        let result = run_text_input(
            &[
                "--input",
                "-",
                "--input-format",
                "roff",
                "--format",
                "json",
                "--display",
                "direct",
                "--word",
                "--search",
                first,
            ],
            &source,
        );
        assert!(result.status.success(), "{head}: {result:?}");
        let response: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(response["total"], 1, "{head}");
    }
}

#[test]
fn bold_device_copies_keep_one_author_until_the_accepted_pass_covers_it() {
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n.ll 140n\n";
    // Fixed ASCII and UTF-8 exercise term.c::encode1()'s earlier bold
    // copy and term_field()'s delayed blank. A blank is only printed
    // over the earlier copy when another device write follows on that pass.
    for font in ["B", "[BI]"] {
        for (after_head, tail, expected) in [
            (
                r#"Ns No "\o' '" No "\l'1n.'\p NEEDLE""#,
                "\nTAILWORD",
                "PR \nNEEDLE TAILWORD\n\nENDTEXT",
            ),
            (
                r#"No "\o' '" No "\l'1n.'\p NEEDLE""#,
                "\nTAILWORD",
                "PRE \nNEEDLE TAILWORD\n\nENDTEXT",
            ),
            (
                r#"No "\o' '" Ns No "\h'-1n'\p NEEDLE""#,
                "\nTAILWORD",
                "PRE\nNEEDLE TAILWORD\n\nENDTEXT",
            ),
            (
                r#"No "\o' '" Ns No "\o' '\p NEEDLE""#,
                "\nTAILWORD",
                "PRE\nNEEDLE TAILWORD\n\nENDTEXT",
            ),
            (r#"Ns No "\o' '" No "\p""#, "", "PRE\n\nENDTEXT"),
        ] {
            let source =
                format!("{header}.No \"\\f{font}PRE\\fR\" {after_head}{tail}\n.Pp\nENDTEXT\n");
            assert_readbacks(&source, expected, None);
        }
    }
}

#[test]
fn native_overstrike_keeps_the_style_and_link_of_a_surviving_bold_copy() {
    let header =
        ".TH SAMPLE 1 \"November 19, 2026\"\n.SH NAME\nsample \\- sample\n.SH DESCRIPTION\n";
    // CVS encode1 emits the first bold copy before the backline and the
    // final copy. A later draw can replace the final copy without replacing
    // the earlier one, whether this author was literal or inside \o.
    for (word, expected, label) in [
        (r"P\fB\o'A'\fR\h'-2n'XYZ", "PAXYZ", None),
        (r"P\f[BI]\o'A'\fR\h'-2n'XYZ", "PAXYZ", None),
        (r"P\fBA\fR\o' '\o' 'Z", "PAZ", None),
        (
            r"P\X'tty: link https://x.io'\fB\o'A'\fR\X'tty: link'\h'-3n'XYZ",
            "PAXYZ",
            Some("A"),
        ),
    ] {
        let source = format!("{header}{word}\n.br\nENDTEXT\n");
        assert_readbacks(&source, &format!("{expected}\nENDTEXT"), label);
        if label.is_none() {
            let ansi = render(&source, "roff", "text", "always", false);
            let bold_copy = if word.contains("[BI]") {
                "\x1b[1m\x1b[3mA\x1b[0m"
            } else {
                "\x1b[1mA\x1b[0m"
            };
            assert!(ansi.contains(bold_copy), "{source}");
        }
        assert_eq!(search_visible_word(&source, expected), 1, "{source}");
    }
}

#[test]
fn empty_overstrike_uses_the_live_entry_slots_across_words() {
    let header = ".Dd November 19, 2026\n.Dt SAMPLE 1\n.Os\n.Sh NAME\n.Nm sample\n.Nd sample\n.Sh DESCRIPTION\n";
    // term_word trims the native buffer after every overstrike. A surviving
    // device space can belong to earlier operands without either current
    // automatic entry spacing or authored output for that cell.
    for (word, expected) in [
        (r#".No P No "\o' '" No "\o' '\o' 'Z""#, "P Z"),
        (r#".No P No "\o' '" No "\o' 'Z""#, "PZ"),
        (r#".No P No "\o' '" No Z"#, "P Z"),
        (r#".No P Ns No "\o' '" Ns No Z"#, "P Z"),
        (r#".No "P\zQ" No "\o''" No Z"#, "PZ"),
        (r#".No "P\zQ" Ns No "\o''" Ns No Z"#, "PZ"),
        (r#".No "P\zQ" Ns No "\o''" Ns No "\o' 'Z""#, "PQZ"),
        (r#".No "P\fBA\fR" No "\:" No "\o' 'Z""#, "PA Z"),
        (r#".No "P\fBA\fR" No "\&" No "\o' '\o' 'Z""#, "PA Z"),
    ] {
        let source = format!("{header}STARTFLAG\n.br\n{word}\n.br\nENDMARK\n");
        assert_readbacks(&source, &format!("STARTFLAG\n{expected}\nENDMARK"), None);
        assert_eq!(
            search_visible_word(&source, "PZ"),
            u64::from(expected == "PZ"),
            "{source}"
        );
    }
}

#[test]
fn native_backspace_only_removes_a_blank_when_its_replacement_is_accepted() {
    // With BACKBEFORE, encode1 can replace the SP directly or step
    // over it after an ignored ASCII_BREAK cell. The next glyph
    // still has to be accepted by term_fill on the same pass.
    for (arm, separator, expected) in [
        ("", r"\p T", &["PREB", "T"][..]),
        ("", r" \:T", &["PREB T"][..]),
        (r"\z", r"\p T", &["PREBT"][..]),
        (r"\z", r" \:T", &["PREBT"][..]),
    ] {
        let word = format!("PRE{arm}\\o'AB'{separator}");
        for body in [
            word.clone(),
            format!(".Bd -literal -compact\n{word}\n.Ed"),
            format!(".TS\nl.\n{word}\nWWWWWWWWWWWWWWWW\n.TE"),
        ] {
            let source = format!("{FIELD_PROBE}STARTFLAG\n.br\n{body}\n.br\nENDFLAG\n");
            let markdown = render(&source, "roff", "markdown", "never", false);
            for text in [
                render(&source, "roff", "text", "never", false),
                render(&markdown, "markdown", "text", "never", false),
            ] {
                let rows: Vec<_> = description(&text).lines().map(str::trim).collect();
                let mut wanted = vec!["STARTFLAG"];
                wanted.extend(expected);
                if body.starts_with(".TS") {
                    wanted.push("WWWWWWWWWWWWWWWW");
                }
                wanted.push("ENDFLAG");
                assert_eq!(rows, wanted, "{source}");
            }
        }
    }
}

#[test]
fn live_markers_and_backspaces_commit_the_same_text_and_link_owners() {
    // CVS term_word writes \p into the buffer. term_fill reads the
    // marker after all writes, including writes reaching a preceding word.
    for (body, rendered, visible, hidden) in [
        (
            r#".No "\zA\p\X'tty: link https://one.test'B\X'tty: link'" No "\h'-3n'T""#,
            "T",
            "T",
            "B",
        ),
        (r#".No "A\zB\pC" No "\h'-2n'T""#, "AT", "AT", "ABC"),
        (
            r#".No PA No "B\pC" No "\h'-5n'T""#,
            "PATBC",
            "PATBC",
            "PA BC",
        ),
        (
            r#".No PA No "B\zC D" No "\h'-5n'T""#,
            "PATBCD",
            "PATBCD",
            "BCD",
        ),
        (r#".No "\pAB" No T Ns No "\h'-3n'Y""#, "AY\nT", "AY", "AB"),
        (r#".No "A\pB" No T Ns No "\h'-3n'Y""#, "AY\nT", "AY", "AB"),
        (
            r#".No "A\pB\:C" No T Ns No "\h'-3n'Y""#,
            "AB\nY T",
            "AB",
            "ABC",
        ),
    ] {
        let source = format!("{EDITABLE_FIELD}{body}\n");
        assert_readbacks(&source, rendered, None);
        assert_eq!(search_visible_word(&source, visible), 1, "{source}");
        assert_eq!(search_visible_word(&source, hidden), 0, "{source}");
        assert!(
            document_links(&source, "roff")
                .iter()
                .all(|(_, text)| text.is_empty())
        );
    }
}

#[test]
fn an_inherited_marker_keeps_the_pending_glyph_before_a_discretionary_break() {
    // CVS term_fill reads both words in one field. ASCII_BREAK ends the
    // pending \p pass, while NBRZW leaves the overstrike on the same row.
    for (word, expected, link) in [
        (r#".No "\pB" No "\zD" Ns No "\:C""#, "B\nC", false),
        (
            r#".No "\pB" No "\X'tty: link https://one.test'\zD\X'tty: link'" Ns No "\:C""#,
            "B\nC",
            false,
        ),
        (r#".No "\zA\p" No "\zB\:C""#, "AB\nC", false),
        (r#".No "\zA\p" Ns No "\z\:C""#, "A\nC", false),
        (r#".No "\zA\p" Ns No "\z" Ns No "\:C""#, "A\nC", false),
        (r#".No "\zA\p" Ns No "\:C""#, "A\nC", false),
        (r#".No "\zA\p\:C""#, "A\nC", false),
        (r#".No "\zA\p" Ns No "\&C""#, "C", false),
        (r#".No "\zA\p" No "\:C""#, "A\nC", false),
        (
            r#".No "\X'tty: link https://one.test'\zA\p\X'tty: link'" Ns No "\:C""#,
            "A\nC",
            true,
        ),
        (
            r#".Sy "\X'tty: link https://one.test'\zA\p\X'tty: link'" Ns No "\:C""#,
            "A\nC",
            true,
        ),
    ] {
        let source = format!("{EDITABLE_FIELD}{word}\n");
        assert_readbacks(&source, expected, None);
        let markdown = render(&source, "roff", "markdown", "never", false);
        for (input, format) in [(source.as_str(), "roff"), (markdown.as_str(), "markdown")] {
            assert_eq!(
                search_visible_word_with_format(input, format, "A"),
                u64::from(expected.starts_with("A\n"))
            );
            assert_eq!(
                search_visible_word_with_format(input, format, "AB"),
                u64::from(expected.starts_with("AB\n"))
            );
            assert_eq!(search_visible_word_with_format(input, format, "DC"), 0);
            assert_eq!(search_visible_word_with_format(input, format, "AC"), 0);
            let targets = document_links(input, format);
            if link {
                assert_eq!(
                    targets,
                    [(
                        LinkTarget::External {
                            uri: "https://one.test".into()
                        },
                        "A".into()
                    )]
                );
            } else {
                assert_eq!(targets, Vec::<(LinkTarget, String)>::new());
            }
        }
    }
}

#[test]
fn an_accepted_backline_does_not_remove_the_first_glyph_after_its_blank() {
    // encode1() appends BS rather than removing a raw byte when the last
    // input is ASCII_BREAK. The accepted field retires the covered space
    // exactly once, keeping B in the original source or link label.
    for (operand, suffix) in [
        (r#".No "  \:BETA""#, ""),
        (r#".Lk https://link.test "  \:BETA""#, ": https://link.test"),
    ] {
        let source = format!(
            r#"{EDITABLE_FIELD}.No "ALPHA\zX\p\: "
{operand}
.br
.No OMEGA
"#
        );
        let markdown = render(&source, "roff", "markdown", "never", false);
        for (input, format) in [(source.as_str(), "roff"), (markdown.as_str(), "markdown")] {
            let text = render(input, format, "text", "never", false);
            assert_eq!(
                description(&text)
                    .lines()
                    .map(str::trim)
                    .collect::<Vec<_>>(),
                ["ALPHAX", &format!("BETA{suffix}"), "OMEGA"],
                "{source}"
            );
            assert_eq!(search_visible_word_with_format(input, format, "BETA"), 1);
            assert_eq!(search_visible_word_with_format(input, format, "ETA"), 0);
        }
    }
}
