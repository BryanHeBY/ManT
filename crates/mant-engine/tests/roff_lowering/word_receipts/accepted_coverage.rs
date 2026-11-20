//! The accepted device row decides the surviving text and annotation owner.

use super::{Labels, Visit, description_rows, json_string_roundtrip};
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};

const REPLAY_HEADER: &str =
    ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd example\n.Sh DESCRIPTION\n";

#[test]
fn appending_a_marker_keeps_earlier_slot_owners_available_to_the_next_word() {
    // Fixed term_field() paints one accepted interval for the first two
    // variants and two intervals for the third. term_word() can overwrite
    // the last marker without restoring a covered glyph or its link.
    for (body, rows, linked) in [
        (
            r#".Sy "\zC\pD\zJ" No "\&\h'-2n'\pF" Ns No "H\p" Ns No "\h'-1n'X""#,
            &["DFHX"][..],
            false,
        ),
        (
            r#".Sy "\z\X'tty: link https://c.test'C\X'tty: link'\pD\zJ" No "\&\h'-2n'\pF" Ns No "H\p" Ns No "\h'-1n'\X'tty: link https://x.test'X\X'tty: link'""#,
            &["DFHX"],
            true,
        ),
        (
            r".Em D\zJ No \h'-3n'\&\pF Em H\p Ns No \h'-1n'X",
            &["DF", "HX"],
            false,
        ),
    ] {
        let source = format!("{REPLAY_HEADER}{body}\n");
        let native =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let markdown = mant_codec::encode::render_markdown(&native);
        let restored = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for content in [&native, &restored] {
            assert_eq!(
                description_rows(&mant_render::render_query_man(content), &source),
                rows,
                "{source}"
            );
            for word in ["DFHX", "DF", "HX", "CDJFHX"] {
                let hits = mant_query::search_query(
                    content,
                    &SearchQuery {
                        pattern: word.into(),
                        syntax: SearchSyntax::Literal,
                        case: SearchCase::Sensitive,
                        scope: SearchScope::Visible,
                        word: true,
                        context_lines: 0,
                        limit: 10,
                        offset: 0,
                    },
                )
                .unwrap();
                assert_eq!(
                    hits.total,
                    u32::from(rows.contains(&word)),
                    "{word}: {source}"
                );
            }
            let mut labels = Labels::default();
            labels.visit_document(content.document.as_ref().unwrap());
            let links: Vec<_> = linked
                .then(|| ("https://x.test".into(), "X".into()))
                .into_iter()
                .collect();
            assert_eq!(labels.0, links, "{source}");
        }
    }
}

#[test]
fn replayed_automatic_spacing_traverses_the_existing_native_slot() {
    // term.c::bufferc() advances over an occupied cell without replacing
    // it with SP. Both fixed devices accept one pass and print DFX.
    let source = format!(
        "{REPLAY_HEADER}{}\n",
        r#".Sy "\zC\pD\zJ" No "\&\h'-2n'\pF" Em "H" Ns No "\h'-1n'X""#
    );
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let markdown = mant_codec::encode::render_markdown(&content);
    let restored = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for document in [&content, &restored] {
        assert_eq!(
            description_rows(&mant_render::render_query_man(document), &source),
            ["DFX"]
        );
        for (word, total) in [("DFX", 1), ("DF", 0), ("X", 0)] {
            let hits = mant_query::search_query(
                document,
                &SearchQuery {
                    pattern: word.into(),
                    syntax: SearchSyntax::Literal,
                    case: SearchCase::Sensitive,
                    scope: SearchScope::Visible,
                    word: true,
                    context_lines: 0,
                    limit: 10,
                    offset: 0,
                },
            )
            .unwrap();
            assert_eq!(hits.total, total, "{word}: {markdown}");
        }
    }
}

#[test]
fn replay_only_carries_the_cursor_of_a_proved_native_suffix() {
    // term_fill() accepts these intervals after the final overwrite. The
    // first, third and fourth keep a real break between their device rows.
    for (body, rows) in [
        (
            r#".No "A\zB" Em "\zCD" No "\:\h'-3n'\pF" No "\f[BI]H\fP" Ns Em "\h'-3n'\&X""#,
            &["ABF", "X"][..],
        ),
        (
            r#".No "A\zB" Ns Sy "\zCD" No "\&\h'-4n'\pF" No "H J" Ns Em "\h'-4n'\zWX""#,
            &["ADFX"],
        ),
        (
            r#".Sy "AB" Sy "\zC\pD\p" Em "\h'-5n'\pF" Em "\^H" Ns Em "\h'-3n'\zWX""#,
            &["AB F", "X"],
        ),
        (
            r#".Sy "\zA" Ns Em "\zCD" Ns Em "\h'-5n'\pF" Em "\:H" Ns Sy "\h'-1n'X""#,
            &["FD", "X"],
        ),
    ] {
        let source = format!("{REPLAY_HEADER}{body}\n");
        let native =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let markdown = mant_codec::encode::render_markdown(&native);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for document in [&native, &readback] {
            assert_eq!(
                description_rows(&mant_render::render_query_man(document), &source),
                rows,
                "{source}"
            );
        }
    }
}

#[test]
fn a_replayed_styled_suffix_keeps_the_earlier_cover() {
    // Pinned term.c::encode1() paints D over C, then ESCAPE_HORIZ moves
    // the cursor without truncating the stored copies. The fixed ASCII
    // and UTF-8 devices both put a single space between D and a later H.
    let cases = ["No", "Sy", "Em"].into_iter().flat_map(|style| {
        ["", r"\&", r"\:"].into_iter().flat_map(move |control| {
            ["", r#" No "H""#]
                .into_iter()
                .map(move |tail| (style, control, tail))
        })
    });
    for (style, control, tail) in cases {
        let source =
            format!(r#"{REPLAY_HEADER}.No "\zA" {style} "\zC{control}D" No "\h'-1n'"{tail}"#);
        let loaded = mant_loader::load_roff_bytes(format!("{source}\n").as_bytes()).unwrap();
        let native = json_string_roundtrip(&loaded);
        let markdown = mant_codec::encode::render_markdown(&native);
        let reloaded = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for content in [&native, &reloaded] {
            let rendered = mant_render::render_query_man(content);
            let expected = if tail.is_empty() { "AD" } else { "AD H" };
            assert_eq!(description_rows(&rendered, &source), [expected], "{source}");
            for (word, total) in [("AD", 1), ("ACD", 0), ("C", 0)] {
                let hits = mant_query::search_query(
                    content,
                    &SearchQuery {
                        pattern: word.into(),
                        syntax: SearchSyntax::Literal,
                        case: SearchCase::Sensitive,
                        scope: SearchScope::Visible,
                        word: true,
                        context_lines: 0,
                        limit: 10,
                        offset: 0,
                    },
                )
                .unwrap();
                assert_eq!(hits.total, total, "{source} {word}");
            }
        }
    }
}

#[test]
fn replayed_cover_drops_only_the_replaced_glyphs_link() {
    // Fixed terminal devices agree on D's bold overstrike and on AD H.
    // The tty annotations mark each source glyph independently.
    let source = format!(
        "{REPLAY_HEADER}{}\n",
        r#".No "\z\X'tty: link https://a.test'A\X'tty: link'" Sy "\z\X'tty: link https://c.test'C\X'tty: link'\:\X'tty: link https://d.test'D\X'tty: link'" No "\h'-1n'" No "\X'tty: link https://h.test'H\X'tty: link'""#,
    );
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let native = json_string_roundtrip(&loaded);
    let markdown = mant_codec::encode::render_markdown(&native);
    let reloaded = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for content in [&native, &reloaded] {
        let rendered = mant_render::render_query_man(content);
        assert_eq!(description_rows(&rendered, &source), ["AD H"]);
        let mut labels = Labels::default();
        labels.visit_document(content.document.as_ref().unwrap());
        assert_eq!(
            labels.0,
            [
                ("https://a.test".into(), "A".into()),
                ("https://d.test".into(), "D".into()),
                ("https://h.test".into(), "H".into()),
            ]
        );
        let export = mant_codec::encode::render_markdown(content);
        assert!(export.contains("[**D**](https://d.test)"), "{export}");
    }
}

#[test]
fn a_replayed_word_keeps_the_break_before_its_font_decoration() {
    // Fixed term_flushln() skips the separator and restarts before H.
    // encode1() may write font decoration before the first visible H.
    for lead in ["No", "Sy", "Em"] {
        for middle in ["No", "Sy", "Em", "Ns No", "Ns Sy"] {
            for tail in ["No", "Sy", "Em"] {
                for first in ["A", "AB"] {
                    let source = format!(
                        "{REPLAY_HEADER}{}\n",
                        format_args!(
                            r#".{lead} "{first}" {middle} "\zC\pD" No "\h'-1n'" {tail} "H""#
                        )
                    );
                    let gap = if middle.starts_with("Ns ") { "" } else { " " };
                    let expected = format!("{first}{gap}D");
                    let native = json_string_roundtrip(
                        &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
                    );
                    let markdown = mant_codec::encode::render_markdown(&native);
                    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
                    for content in [&native, &readback] {
                        let rendered = mant_render::render_query_man(content);
                        assert_eq!(
                            description_rows(&rendered, &source),
                            [&expected, "H"],
                            "{source}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_replaced_word_cannot_own_the_restart_of_an_accepted_pass() {
    // Fixed term_field() prints the retained C before the break. The E
    // word's range crosses the restart, but its projected glyph is gone.
    let source = format!(
        "{REPLAY_HEADER}{}\n",
        r#".No "A \zB" Em "C\p\z D" Ns Em "\h'-2n'\:E" Ns Em "\h'-1n'H""#,
    );
    let native = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let markdown = mant_codec::encode::render_markdown(&native);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for content in [&native, &readback] {
        let rendered = mant_render::render_query_man(content);
        assert_eq!(description_rows(&rendered, &source), ["A BC", "H"]);
    }
}

#[test]
fn deferred_native_passes_survive_replay_across_invisible_cells_and_style() {
    // Fixed ASCII and UTF-8 agree. term_word() writes an entry blank
    // before applying BACKBEFORE, and term_fill() later consumes the
    // authored marker. Zero-width controls also participate in the replay.
    for lead in [r"A\p\z B", r"A\p\z  B"] {
        for control in ["", r"\&", r"\:", r"\h'0n'", r"\fI\fP"] {
            let source = format!(r#"{REPLAY_HEADER}.No "{lead}" No "\zC{control}\h'-2n'""#);
            let loaded = mant_loader::load_roff_bytes(format!("{source}\n").as_bytes()).unwrap();
            let native = json_string_roundtrip(&loaded);
            let markdown = mant_codec::encode::render_markdown(&native);
            let reloaded = mant_loader::load_markdown_text(&markdown, None).unwrap();
            for content in [&native, &reloaded] {
                let rendered = mant_render::render_query_man(content);
                assert_eq!(
                    description_rows(&rendered, &source),
                    ["A", "BC"],
                    "{source}"
                );
            }
        }
    }
}

#[test]
fn repeated_rewinds_keep_accepted_rows_in_each_structural_owner() {
    // Fixed term_flushln() accepts all three rows for each of these
    // containers. Its final write replaces E in the first case, while
    // the styled D survives the raw decoration writes in the second.
    for (body, expected) in [
        (
            r#".Sy "A\p\z B\&" Ns No "\zC\p D" No "\:E" Ns No "\h'-1n'H""#,
            ["A", "CD", "H"],
        ),
        (
            r#".No "A\p\z B" Sy "C\p\z  D" No "\h'-2n'E" Ns No "H""#,
            ["A", "BC", "DEH"],
        ),
    ] {
        for (prefix, suffix) in [
            ("", ""),
            (".Bd -literal -compact\n", ".Ed\n"),
            (".Bd -unfilled -compact\n", ".Ed\n"),
            (".Bl -tag -width aa -compact\n.It ITEM\n", ".El\n"),
        ] {
            let source = format!("{REPLAY_HEADER}{prefix}{body}\n{suffix}");
            let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let native = json_string_roundtrip(&loaded);
            let markdown = mant_codec::encode::render_markdown(&native);
            let reloaded = mant_loader::load_markdown_text(&markdown, None).unwrap();
            for content in [&native, &reloaded] {
                let rendered = mant_render::render_query_man(content);
                let rows = description_rows(&rendered, &source);
                let rows = if prefix.starts_with(".Bl") {
                    let (label, rows) = rows.split_first().unwrap();
                    assert!(label.ends_with("ITEM"), "{source}: {label}");
                    rows
                } else {
                    &rows
                };
                assert_eq!(
                    rows.iter().map(|row| row.trim()).collect::<Vec<_>>(),
                    expected,
                    "{source}"
                );
            }
        }
    }
}

#[test]
fn repainted_final_glyph_keeps_its_link_and_its_own_search_word() {
    // The fixed devices agree on A, CD and H. H overwrites E in
    // term_word(), and its separate tty annotation does not own A.
    let source = concat!(
        ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd example\n.Sh DESCRIPTION\n",
        r#".Sy "\X'tty: link https://one.test'A\X'tty: link'\p\z B\&" Ns No "\zC\p D" No "\:E" Ns No "\h'-1n'\X'tty: link https://two.test'H\X'tty: link'""#,
        "\n",
    );
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let native = json_string_roundtrip(&loaded);
    let markdown = mant_codec::encode::render_markdown(&native);
    let reloaded = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for content in [&native, &reloaded] {
        let rendered = mant_render::render_query_man(content);
        assert_eq!(description_rows(&rendered, source), ["A", "CD", "H"]);
        let mut labels = Labels::default();
        labels.visit_document(content.document.as_ref().unwrap());
        assert_eq!(
            labels.0,
            [
                ("https://one.test".into(), "A".into()),
                ("https://two.test".into(), "H".into()),
            ]
        );
        for (probe, total) in [("A", 1), ("CD", 1), ("H", 1), ("E", 0), ("EH", 0)] {
            let hits = mant_query::search_query(
                content,
                &SearchQuery {
                    pattern: probe.into(),
                    syntax: SearchSyntax::Literal,
                    case: SearchCase::Sensitive,
                    scope: SearchScope::Visible,
                    word: true,
                    context_lines: 0,
                    limit: 10,
                    offset: 0,
                },
            )
            .unwrap();
            assert_eq!(hits.total, total, "{probe}: {markdown}");
        }
    }
}

#[test]
fn coverage_uses_the_same_printed_row_and_keeps_the_surviving_link() {
    // The fixed ASCII and UTF-8 devices agree on these exact operands.
    // term_fill() chooses the interval before term_field() emits its
    // backspaces. HORIZ changes col without truncating that interval.
    let header = ".Dd November 19, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd example\n.Sh DESCRIPTION\n";
    let uri = "https://example.invalid/c";
    for (body, rows, linked) in [
        (
            r#".No "\zB\p" No "\h'1n'\zC\p" Ns No "\:D""#,
            &["B", "C", "D"][..],
            false,
        ),
        (
            r#".No "\zB\p" No "\h'1n'\X'tty: link https://example.invalid/c'\zC\X'tty: link'\p" Ns No "\:D""#,
            &["B", "C", "D"],
            true,
        ),
        (
            r#".No "\zA\p\:B\&" Ns No "\h'-1n'\zC" Ns No "\:D""#,
            &["A", "BD"],
            false,
        ),
        (
            r#".No "\zA\p\:B\&" Ns No "\h'-1n'\X'tty: link https://example.invalid/c'\zC\X'tty: link'" Ns No "\:D""#,
            &["A", "BD"],
            false,
        ),
        (
            r#".No "P\p\z Q" Ns No "\:A" No "\h'-2n'""#,
            &["P", "A"],
            false,
        ),
        (
            r#".No "A\zB\p\:\&" No "\h'-1n'\zC" Ns No "\:D""#,
            &["AB", "D"],
            false,
        ),
    ] {
        let source = format!("{header}{body}\n");
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let native = json_string_roundtrip(&loaded);
        let markdown = mant_codec::encode::render_markdown(&native);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for content in [&native, &readback] {
            let rendered = mant_render::render_query_man(content);
            assert_eq!(description_rows(&rendered, &source), rows, "{body}");
            let mut labels = Labels::default();
            labels.visit_document(content.document.as_ref().unwrap());
            if linked {
                assert_eq!(labels.0, [(uri.into(), "C".into())], "{body}");
            } else {
                assert!(labels.0.is_empty(), "{body}: {:?}", labels.0);
            }
            for probe in ["AB", "A", "BD", "BCD", "C", "QA", "Q"] {
                let hits = mant_query::search_query(
                    content,
                    &SearchQuery {
                        pattern: probe.into(),
                        syntax: SearchSyntax::Literal,
                        case: SearchCase::Sensitive,
                        scope: SearchScope::Visible,
                        word: true,
                        context_lines: 0,
                        limit: 10,
                        offset: 0,
                    },
                )
                .unwrap();
                assert_eq!(
                    hits.total,
                    u32::from(rows.contains(&probe)),
                    "{body}: {probe}"
                );
            }
        }
    }
}

#[test]
fn rewinding_into_font_decoration_does_not_duplicate_a_covered_glyph() {
    // In both pinned devices, encode1() writes the bold copies of A
    // before the leading negative motion of the next operand. At least
    // one A copy survives. The next operand's C is covered by D.
    let header = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n";
    for font_macro in ["BR", "BI"] {
        let source = format!(
            "{header}.{font_macro} \"D\\p\\z A\" \"\\h'-1n'\\zC\" \"\\:D\"\n.SH ENDTEST\nzz\n"
        );
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let native = json_string_roundtrip(&loaded);
        let text = mant_render::render_query_man(&native);
        let (_, tail) = text.split_once("\nFIRST\n").unwrap();
        let (rows, _) = tail.split_once("\n\nENDTEST\n").unwrap();
        assert_eq!(rows, "D\nAD", "{source}");
    }
}

#[test]
fn a_marker_in_the_rewound_word_preserves_the_cursor_for_the_next_word() {
    // The exact CVS ASCII/UTF-8 runs write Y into the remaining live
    // slots after T. ESCAPE_HORIZ changes col, while term_fill() reads
    // through lastcol. Appending Y past the old marker would reject it.
    let source = concat!(
        ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n",
        r#".No "\pP\z\p Q" Ns No "\p \&A\:B\p" No "\p\:\h'-4n'T" No Y"#,
        "\n",
    );
    let original = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let native = json_string_roundtrip(&original);
    let text = mant_render::render_query_man(&native);
    assert_eq!(description_rows(&text, source), ["P", "Q", "ABT Y"]);
}

#[test]
fn retreat_within_the_next_word_keeps_its_earlier_annotation() {
    // Both pinned devices print A on the first row and BDC on the next.
    // The motion in term_word() follows the current word's own BACKBEFORE
    // glyph. Padding deferred by the preceding \p cannot consume that glyph.
    let source = concat!(
        ".Dd November 19, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd example\n.Sh DESCRIPTION\n",
        r#".No "\X'tty: link https://a.example'A\X'tty: link'\p " Ns No "\X'tty: link https://b.example'B\X'tty: link'\zC\h'-1n'D""#,
        "\n",
    );
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let native = json_string_roundtrip(&loaded);
    let markdown = mant_codec::encode::render_markdown(&native);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for content in [&native, &readback] {
        let text = mant_render::render_query_man(content);
        assert_eq!(description_rows(&text, source), ["A", "BDC"]);
        let mut labels = Labels::default();
        labels.visit_document(content.document.as_ref().unwrap());
        assert_eq!(
            labels.0,
            [
                ("https://a.example".into(), "A".into()),
                ("https://b.example".into(), "B".into())
            ]
        );
    }
}

#[test]
fn motion_and_automatic_padding_do_not_spend_a_glyphs_accepted_range() {
    // CVS term.c::term_fill and term_field accept the same rows on ASCII
    // and UTF-8, including the leading NBRZW that comes with a link range.
    // Relative forward motion traverses, rather than replacing, old slots.
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd example\n.Sh DESCRIPTION\n";
    let uri = "https://one.test/c";
    for (body, rows, linked) in [
        (
            r#".No "\zB" No "\h'2n'\zC" Ns No "\:E""#,
            &["B E"][..],
            false,
        ),
        (
            r#".No "\zB" No "\h'2n'\z\X'tty: link https://one.test/c'C\X'tty: link'" Ns No "\:E""#,
            &["B E"],
            false,
        ),
        (r#".Sy "\zB" No "\h'1n'\h'-2n'\p C""#, &["B", "C"], false),
        (
            r#".Sy "\zB" No "\h'1n'\h'-2n'\p \X'tty: link https://one.test/c'C\X'tty: link'""#,
            &["B", "C"],
            true,
        ),
        (r#".Sy "A\&" Sy "\h'-2n'\h'1n'C""#, &["AC"], false),
        (r#".Sy "\&" Sy "\h'-2n'\h'1n'\p C""#, &["", "C"], false),
        (
            r#".No "A\zB" Sy "\h'1n'\h'-2n'" Ns No "\:E""#,
            &["AE"],
            false,
        ),
        (
            r#".No "A\zB" Sy "\h'1n'\h'-2n'" Ns No "\p E""#,
            &["A", "E"],
            false,
        ),
    ] {
        let source = format!("{header}{body}\n");
        let native =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let markdown = mant_codec::encode::render_markdown(&native);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for content in [&native, &readback] {
            let rendered = mant_render::render_query_man(content);
            assert_eq!(description_rows(&rendered, &source), rows, "{body}");
            let mut labels = Labels::default();
            labels.visit_document(content.document.as_ref().unwrap());
            let expected = if linked {
                vec![(uri.into(), "C".into())]
            } else {
                Vec::new()
            };
            assert_eq!(labels.0, expected, "{body}");
            for probe in ["C", "E", "CE", "AC", "A", "AE", "ABE"] {
                let hits = mant_query::search_query(
                    content,
                    &SearchQuery {
                        pattern: probe.into(),
                        syntax: SearchSyntax::Literal,
                        case: SearchCase::Sensitive,
                        scope: SearchScope::Visible,
                        word: true,
                        context_lines: 0,
                        limit: 10,
                        offset: 0,
                    },
                )
                .unwrap();
                assert_eq!(
                    hits.total,
                    u32::from(
                        rows.iter()
                            .flat_map(|row| row.split_whitespace())
                            .any(|word| word == probe)
                    ),
                    "{body}: {probe}"
                );
            }
        }
    }
}

#[test]
fn consecutive_rewinds_remove_the_previous_projection_of_a_buffered_blank() {
    // CVS term.c::term_word() rewrites the separator at Y's landing.
    // One native slot may have projected both a word separator and a
    // borrowed prefix. The rewrite must retire these two representations.
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";
    for (body, linked) in [
        (
            r#".No "P\zQ" No "\p A\p\X'tty: link https://two.test'B\X'tty: link'" No "\h'-7n'T" Ns No "\h'-2n'Y""#,
            true,
        ),
        (
            r#".No "P \h'-1n'Q" No "\p\&\pA\zB" No "\h'-5n'T" Ns No "\h'-2n'Y""#,
            false,
        ),
    ] {
        let source = format!("{header}{body}\n");
        let native =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let markdown = mant_codec::encode::render_markdown(&native);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for content in [&native, &readback] {
            let rendered = mant_render::render_query_man(content);
            assert_eq!(description_rows(&rendered, &source), ["PQYTAB"], "{body}");
            let mut labels = Labels::default();
            labels.visit_document(content.document.as_ref().unwrap());
            let expected = linked.then(|| ("https://two.test".into(), "B".into()));
            assert_eq!(labels.0, expected.into_iter().collect::<Vec<_>>(), "{body}");
        }
    }
}

#[test]
fn a_buffered_gap_can_print_with_a_later_words_glyph() {
    // CVS term_field() releases the gap at the next printing cell on
    // that pass. Closing the pass or reaching its end does not print it.
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";
    for (body, rows, target) in [
        (
            r#".Sy "AB" No "\h'-2n'\&\h'1n'C" Ns No "\h'-1n'E""#,
            &["A E"][..],
            None,
        ),
        (
            r#".Sy "AB" No "\h'-2n'\:\h'1n'C" Ns No "\h'-1n'E""#,
            &["A E"],
            None,
        ),
        (
            r#".Sy "AB" No "\h'-2n'\&\h'1n'C" Ns No "\h'-1n'\X'tty: link https://two.test'E\X'tty: link'""#,
            &["A E"],
            Some(("https://two.test", "E")),
        ),
        (
            r#".Sy "AB" No "\h'-2n'\&\h'1n'C" Ns No "\h'-1n'\&""#,
            &["AB"],
            None,
        ),
        (
            r#".Sy "AB" No "\h'-2n'\&\h'1n'C" Ns No "\p E""#,
            &["A C", "E"],
            None,
        ),
        (
            r#".Sy "AB" No "\h'-2n'\&\h'1n'C" Ns No "\h'-1n'E" Ns No "\h'-1n'F""#,
            &["A F"],
            None,
        ),
    ] {
        let source = format!("{header}{body}\n");
        let native =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let markdown = mant_codec::encode::render_markdown(&native);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for content in [&native, &readback] {
            let rendered = mant_render::render_query_man(content);
            assert_eq!(description_rows(&rendered, &source), rows, "{body}");
            let mut labels = Labels::default();
            labels.visit_document(content.document.as_ref().unwrap());
            let expected: Vec<_> = target
                .into_iter()
                .map(|(uri, label)| (uri.into(), label.into()))
                .collect();
            assert_eq!(labels.0, expected, "{body}");
            for probe in ["AE", "E", "F"] {
                let result = mant_query::search_query(
                    content,
                    &SearchQuery {
                        pattern: probe.into(),
                        syntax: SearchSyntax::Literal,
                        case: SearchCase::Sensitive,
                        scope: SearchScope::Visible,
                        word: true,
                        context_lines: 0,
                        limit: 10,
                        offset: 0,
                    },
                )
                .unwrap();
                let expected = rows
                    .iter()
                    .flat_map(|row| row.split_whitespace())
                    .any(|word| word == probe);
                assert_eq!(result.total, u32::from(expected), "{body}: {probe}");
            }
        }
    }
}

#[test]
fn font_register_events_do_not_sever_the_live_cursor() {
    // The CVS font cases in term_word() update only registers. Each
    // input below preserves the same bytes at the two accepted passes.
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";
    for operand in [
        r"\zC\h'-2n'",
        r"\zC\fI\fP\h'-2n'",
        r"\z\fI\fPC\h'-2n'",
        r"\fI\fP\zC\h'-2n'",
        r"\zC\h'-2n'\fI\fP",
    ] {
        let source = format!("{header}.Sy \"A\\p\\z B\" No \"{operand}\" No \"\\p E\"\n");
        let native =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let rendered = mant_render::render_query_man(&native);
        assert_eq!(
            description_rows(&rendered, &source),
            ["A", "CE"],
            "{source}"
        );
    }
}

#[test]
fn tight_replay_preserves_a_surviving_prior_link_owner() {
    // tty annotations write invisible slots. With both given targets
    // and a tight entry, term_field() prints the previous B and the
    // following D; C at its attempted slot never owns visible ink.
    let header = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";
    for (a, b, c, d) in [('A', 'B', 'C', 'D'), ('Q', 'Q', 'Q', 'Q')] {
        let source = format!(
            "{header}.No \"\\X'tty: link https://one.test'{a}\\X'tty: link'\\p\\z \\X'tty: link https://two.test'{b}\\X'tty: link'\" Ns No \"\\z{c}\\h'-2n'\" No \"\\:{d}\"\n"
        );
        let native =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let markdown = mant_codec::encode::render_markdown(&native);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for content in [&native, &readback] {
            let rendered = mant_render::render_query_man(content);
            assert_eq!(
                description_rows(&rendered, &source),
                [a.to_string(), format!("{b}{d}")]
            );
            let mut labels = Labels::default();
            labels.visit_document(content.document.as_ref().unwrap());
            assert_eq!(
                labels.0,
                [
                    ("https://one.test".into(), a.to_string()),
                    ("https://two.test".into(), b.to_string())
                ]
            );
        }
    }
}

#[test]
fn a_retreat_can_replace_the_blank_after_an_unflushed_marker() {
    // term_fill() walks the final input buffer. D replaces the ordinary
    // separator before it can trigger the earlier \p; C overprints B.
    let source = concat!(
        ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n",
        r#".No "\X'tty: link https://one.test'A\X'tty: link'\p\z \X'tty: link https://two.test'B\X'tty: link'" Ns No "\zC\h'-2n'" Ns No "\h'-3n'D""#,
        "\n",
    );
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let native = json_string_roundtrip(&content);
    let rendered = mant_render::render_query_man(&native);
    assert_eq!(description_rows(&rendered, source), ["ADC"]);
    let mut labels = Labels::default();
    labels.visit_document(native.document.as_ref().unwrap());
    labels.0.retain(|(_, label)| !label.is_empty());
    assert_eq!(labels.0, [("https://one.test".into(), "A".into())]);
}
