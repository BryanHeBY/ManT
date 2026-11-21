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
fn consecutive_backspaces_cover_glyphs_across_invisible_fill_controls() {
    // term_field() emits both backspaces on the accepted pass across
    // NBRZW and the marker. Both pinned devices paint HX. A tty label
    // on F changes the native writes, and DFHX survives in that control.
    for (f, retained) in [
        ("F", false),
        (r"\X'tty: link https://covered.test'F\X'tty: link'", true),
    ] {
        for (x, linked) in [
            ("X", false),
            (r"\X'tty: link https://kept.test'X\X'tty: link'", true),
        ] {
            let source = format!(
                "{REPLAY_HEADER}{}\n",
                format_args!(
                    r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\z{f}" Em "H\p" Ns No "\h'-1n'{x}""#
                )
            );
            let native =
                json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
            let markdown = mant_codec::encode::render_markdown(&native);
            let restored = mant_loader::load_markdown_text(&markdown, None).unwrap();
            let links: Vec<_> = retained
                .then_some(("https://covered.test", "F"))
                .into_iter()
                .chain(linked.then_some(("https://kept.test", "X")))
                .collect();
            let row = if retained { "DFHX" } else { "HX" };
            for content in [&native, &restored] {
                assert_eq!(
                    description_rows(&mant_render::render_query_man(content), &source),
                    [row],
                    "{source}"
                );
                let mut labels = Labels::default();
                labels.visit_document(content.document.as_ref().unwrap());
                let actual: Vec<_> = labels
                    .0
                    .iter()
                    .map(|(uri, label)| (uri.as_str(), label.as_str()))
                    .collect();
                assert_eq!(actual, links, "{source}");
            }
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

#[test]
fn terminal_marker_uses_buffered_cells_instead_of_the_last_escape_event() {
    // Both pinned devices preserve HJ on one row and the authored gap in
    // H X. term_fill() sees H\p and H \p differently; selecting a font or
    // a zero movement after the marker writes no new glyph.
    for (body, rows, linked) in [
        (
            r#".Sy "\zC\p\zJ" No "\~\h'-9n'\p\&\zF" Ns No "H \p" Ns No "\:""#,
            &["HJ", "\u{a0}"][..],
            None,
        ),
        (
            r#".Em "\zC\pJ" No "\h'-9n'\&\zFG" Em "H \p" Em "\h'-2n'X""#,
            &["H X"],
            None,
        ),
        (
            r#".Em "\zCD\zJ" No "\h'-8n'\:\X'tty: link https://f.test'F\X'tty: link'" Em "H \p" Em "\h'-2n'X""#,
            &["FDH X"],
            Some(("https://f.test", "F")),
        ),
        (
            r#".Sy "\z\X'tty: link https://c.test'C\X'tty: link'\pD\zJ" No "\&\h'-2n'\pF" Ns No "H\p\fP" Ns No "\h'-1n'X""#,
            &["DFHX"],
            None,
        ),
        (
            r".Em D\zJ No \h'-3n'\&\pF Em H\p\fP Ns No \h'-1n'X",
            &["DF", "HX"],
            None,
        ),
    ] {
        assert_device_row(body, rows, linked);
    }
}

#[test]
fn claimed_source_blanks_follow_the_accepted_slots_after_marker_edits() {
    // CVS term_word/term_field. The controls leave the marker at the
    // native write cursor until the next word can overwrite its slots.
    for (spaces, rows) in [(0, ["HX", "X"]), (1, ["H X", "HX"]), (2, ["H  X", "H X"])] {
        for (control, no_space) in [
            ("", false),
            (r"\c", true),
            (r"\h'1n'", false),
            (r"\:", false),
            (r"\fP", false),
            (r"\h'0n'", false),
        ] {
            let blanks = " ".repeat(spaces);
            let body = format!(
                r#".Em "\zC\pJ" No "\h'-9n'\&\zFG" Em "H{blanks}\p{control}" Em "\h'-2n'X""#,
            );
            assert_device_row(&body, &[rows[usize::from(no_space)]], None);
        }
    }
    assert_device_row(
        r#".Sy "\z\X'tty: link https://c.test'C\X'tty: link'\pD\zJ" No "\&\h'-2n'\pF" Ns No "H\p\c" Ns No "\h'-1n'X""#,
        &["DFHX"],
        None,
    );
    assert_device_row(
        r#".Em "\zC\pJ" No "\h'-9n'\&\zFG" Em "H \p\h'1n'" Em "\:\h'-2n'X""#,
        &["H  X"],
        None,
    );
}

#[test]
fn independent_font_runs_preserve_earlier_overprint_owners() {
    // In both devices encode1's decoration copies in an independent
    // suffix do not change the earlier term_field coverage or ink order.
    for (tail, expected) in [
        ("Ns No QQ", "HXQQ"),
        ("Ns Sy QQ", "HXQQ"),
        ("Ns Em QQ", "HXQQ"),
        ("Ns Sy QQ Ns Em RR", "HXQQRR"),
        ("Ns Sy Q Ns Em R", "HXQR"),
        ("Ns Sy QQQQQ", "HXQQQQQ"),
    ] {
        let body =
            format!(r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em "H\p" Ns No "\h'-1n'X" {tail}"#);
        assert_device_row(&body, &[expected], None);
    }
    // The suffix is stored as a range. The 64-cell replay window limits
    // raw placement without disqualifying the earlier accepted overlap.
    for (style, length) in [("Sy", 63), ("Sy", 64), ("Sy", 65), ("Em", 96)] {
        let tail = "Q".repeat(length);
        let body = format!(
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em "H\p" Ns No "\h'-1n'X" Ns {style} {tail}"#
        );
        assert_device_row(&body, &[&format!("HX{tail}")], None);
    }
}

#[test]
fn surviving_owners_follow_device_columns_across_fill_controls() {
    // term.c::term_field() emits both retreats. D is overwritten and X
    // precedes the original F, even when a different owner supplies X.
    for (body, linked) in [
        (
            r#".Sy "\zC\pD\zJ" No "\^\h'-4n'\zF" No "H\p" Ns No "\h'-2n'\&\&X""#,
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em H Ns No "\h'-1n'X""#,
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em H Ns No "\h'-1n'\X'tty: link https://kept.test'X\X'tty: link'""#,
            Some(("https://kept.test", "X")),
        ),
    ] {
        assert_device_row(body, &["XF"], linked);
    }
}

#[test]
fn append_only_tails_do_not_change_the_proved_overprint() {
    // The native 63/64/65-slot passes and UTF-8 variants all print the
    // same left prefix. ASCII transcribes the non-ASCII tail independently.
    let ascii = [48, 49, 50].map(|length| "Q".repeat(length));
    for (suffix, projected) in ascii
        .iter()
        .map(|suffix| (suffix.as_str(), suffix.as_str()))
        .chain([
            ("QQQé", "QQQé"),
            ("QQQΩ", "QQQΩ"),
            ("QQQ界", "QQQ界"),
            (r"QQQ\[u00E9]", "QQQé"),
        ])
    {
        let body =
            format!(r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em "H\p" Ns No "\h'-1n'X{suffix}""#);
        assert_device_row(&body, &[&format!("HX{projected}")], None);
    }
}

#[test]
fn a_hard_row_cut_does_not_consume_the_first_payload_of_the_next_pass() {
    // A resumed formatter word can have a control-only interval in the
    // accepted row and real ink at the start of the next row. In both
    // fixed devices term_fill() keeps that ink after the actual loop cut.
    for (body, rows) in [
        (
            r#".Sy "\zC\pD\zJ" No "\p\h'-4n'\&\p\zFG" Em "H" Ns No "\:\h'-4n'\&X""#,
            &["DG", "XH"][..],
        ),
        (
            r#".Sy "\zC\pD\zJ" Sy "\&\h'-1n'\p\:F\p" No "H\zG\p" Ns No "\h'-2n'X""#,
            &["DJ", "F", "XG"],
        ),
    ] {
        assert_device_row(body, rows, None);
    }
}

fn assert_device_row(body: &str, rows: &[&str], link: Option<(&str, &str)>) {
    let source = format!("{REPLAY_HEADER}{body}\n");
    let native = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let mut content = native;
    for _ in 0..3 {
        let rendered = mant_render::render_query_man(&content);
        assert_eq!(description_rows(&rendered, &source), rows, "{body}");
        for (pattern, total) in rows
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|&word| (word, 1))
            .chain(
                ["FX", "CDFHX", "CDJFHX", "HX", "DFH X"]
                    .into_iter()
                    .filter(|word| !rows.contains(word))
                    .map(|word| (word, 0)),
            )
        {
            let hits = mant_query::search_query(
                &content,
                &SearchQuery {
                    pattern: pattern.into(),
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
            assert_eq!(hits.total, total, "{body}: {pattern}");
        }
        let mut labels = Labels::default();
        labels.visit_document(content.document.as_ref().unwrap());
        let expected = link
            .map(|(uri, value)| (uri.into(), value.into()))
            .into_iter()
            .collect::<Vec<_>>();
        assert_eq!(labels.0, expected, "{body}");
        let markdown = mant_codec::encode::render_markdown(&content);
        content = mant_loader::load_markdown_text(&markdown, None).unwrap();
    }
}

#[test]
fn accepted_drawing_cells_keep_their_rows_through_motion_and_appended_styles() {
    // CVS term_fill() accepts raw intervals before term_field() paints them.
    // A horizontal advance only becomes reading space when it is printed;
    // later font switches, NBRZW and split passes do not revive covered ink.
    for (body, rows, link) in [
        (
            r#".Em "\zC\pJ" No "\h'-9n'\&\zFG" Em "H \p\h'1n'" Em "\:\h'-1n'X""#,
            &["H", "X"][..],
            None,
        ),
        (
            r#".Em "\zC\pJ" No "\h'-9n'\&\zFG" Em "H \p\h'1n'" Em "\:\h'-1n'\X'tty: link https://x.test'X\X'tty: link'""#,
            &["H", "X"][..],
            Some(("https://x.test", "X")),
        ),
        (
            r#".Em "\zC\pJ" No "\h'-9n'\&\zFG" Em "H\p\h'2n'" Em "\h'-2n'X""#,
            &["H X"][..],
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em "H\p" Ns No "\h'-1n'X" Ns Sy "QQ\&""#,
            &["HXQQ"][..],
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em "H\p" Ns No "\h'-1n'X" Ns Sy "Q\fRQ""#,
            &["HXQQ"][..],
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em "H\p" Ns No "\h'-1n'X" Ns Sy "Q Q""#,
            &["HXQ", "Q"][..],
            None,
        ),
        (
            r#".Sy "\zC\p\X'tty: link https://d.test'D\X'tty: link'\zJ" No "\:\h'-6n'\&\p\zF" Em "H\p" Ns No "\h'-1n'X" Ns Sy "QQ\&""#,
            &["HXQQ"][..],
            None,
        ),
        (r#".Sy "C\zJ" No "\h'-3n'\&""#, &["J"][..], None),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em "H\p\h'2n'" Ns No "\h'-1n'X""#,
            &["H X"][..],
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em "H\h'0n'\p\h'2n'" Ns No "\h'-1n'X""#,
            &["H X"][..],
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em "H\&\p\h'2n'" Ns No "\h'-1n'X""#,
            &["H X"][..],
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Sy "H\p\h'2n'" Ns No "\h'-1n'X""#,
            &["H X"][..],
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Sy "H\h'0n'\p\h'2n'" Ns No "\h'-1n'X""#,
            &["H X"][..],
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Sy "H\&\p\h'2n'" Ns No "\h'-1n'X""#,
            &["H X"][..],
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em "H\h'1n' \p\h'-1n'" Ns No "\h'-1n'X""#,
            &["H X"][..],
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em "H\h'1n'\h'-1n'  \p\h'-1n'" Ns No "\h'-1n'X""#,
            &["H X"][..],
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Sy "H\h'1n' \p\h'-1n'" Ns No "\h'-1n'X""#,
            &["H X"][..],
            None,
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Sy "H\h'1n'\h'-1n'  \p\h'-1n'" Ns No "\h'-1n'X""#,
            &["H X"][..],
            None,
        ),
    ] {
        assert_device_row(body, rows, link);
    }
}

#[test]
fn an_open_head_keeps_its_ink_only_until_the_body_paints_the_same_cells() {
    // Both fixed devices give X ownership of the current column after
    // consecutive backspaces. The F beyond the cursor stays painted if
    // roff_term_pre_br or Pp ends the row before term_field emits BODY.
    for (list, width, shared_gap, body_indent) in [("hang", "4n", 5, 6), ("tag", "12n", 13, 14)] {
        for boundary in ["", ".br\n", ".Pp\n"] {
            let source = format!(
                r#".Dd November 19, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd field probe
.Sh DESCRIPTION
.Bl -{list} -width {width}
.It Xo
.Sy "\zC\pD\zJ" No "\:\h'-6n'\&\p\zF" Em H Ns No "\h'-1n'X"
.Xc
{boundary}.No BODY
.El
.Sh ENDTEST
Done.
"#
            );
            let content =
                json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
            let reading = mant_render::render_query_man(&content);
            let rows = description_rows(&reading, &source);
            let mut expected = Vec::new();
            if boundary.is_empty() {
                expected.push(format!("X{}BODY", " ".repeat(shared_gap)));
            } else {
                expected.push("XF".to_owned());
                if boundary == ".Pp\n" {
                    expected.push(String::new());
                }
                expected.push(format!("{}BODY", " ".repeat(body_indent)));
            }
            assert_eq!(rows, expected, "{source}");
            let mut restored = content;
            for _ in 0..2 {
                let markdown = mant_codec::encode::render_markdown(&restored);
                restored = mant_loader::load_markdown_text(&markdown, None).unwrap();
                let reading = mant_render::render_query_man(&restored);
                let rows = description_rows(&reading, &source);
                let with_body = rows.iter().find(|line| line.contains("BODY")).unwrap();
                if boundary.is_empty() {
                    assert_eq!(
                        with_body
                            .trim_start()
                            .trim_start_matches("• ")
                            .split_whitespace()
                            .collect::<Vec<_>>(),
                        ["X", "BODY"],
                        "{markdown}"
                    );
                    assert!(!rows.iter().any(|line| line.contains("XF")));
                } else {
                    assert_eq!(with_body.trim(), "BODY", "{markdown}");
                    assert!(
                        rows.iter()
                            .any(|line| line.trim().trim_start_matches("• ") == "XF"),
                        "{markdown}"
                    );
                }
            }
        }
    }
}

#[test]
fn an_append_proof_cannot_authorize_a_rewind_across_opaque_field_bytes() {
    // The accepted CVS field still includes HX, G or HX respectively.
    // A preceding suffix proves its decoration but not where those
    // glyphs land after the next term_word() crosses an uncertain slot.
    for (body, kept) in [
        (
            r#".Em "\zC\pJ" No "\:\p\h'-9n'\&\zFG" Em "H \p" Em "\h'-2n'X""#,
            "HX",
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\p\p\h'-10n'\&\p\p\zFG" Em "H\zG" Ns No "\:\h'-4n'\&X""#,
            "G",
        ),
        (
            r#".Sy "\zC\pD\zJ" No "\p\h'-9n'\&\p\zFG" Sy "H\zG" Ns No "\h'-1n'\:\h'-4n'\&X""#,
            "HX",
        ),
    ] {
        let source = format!("{REPLAY_HEADER}{body}\n.Sh ENDTEST\nDone.\n");
        let mut content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        for _ in 0..3 {
            let rendered = mant_render::render_query_man(&content);
            let text = description_rows(&rendered, &source).join("\n");
            let mut visible = text.chars();
            for glyph in kept.chars() {
                assert!(
                    visible.any(|ch| ch == glyph),
                    "missing {glyph}: {source}\n{text}"
                );
            }
            let markdown = mant_codec::encode::render_markdown(&content);
            content = mant_loader::load_markdown_text(&markdown, None).unwrap();
        }
    }
}

const LIVE_REPLAY_CASES: &[(&str, &[&str], Option<&str>)] = &[
    (
        include_str!("live_replay/replay-loss-single-D.1"),
        &["D", "J"],
        None,
    ),
    (include_str!("live_replay/replay-letter.1"), &["A FX"], None),
    (
        include_str!("live_replay/replay-letter-link-F.1"),
        &["A FX"],
        Some("F"),
    ),
    (
        include_str!("live_replay/replay-letter-link-X.1"),
        &["A FX"],
        Some("X"),
    ),
    (include_str!("live_replay/replay-gap.1"), &["A  XG"], None),
    (
        include_str!("live_replay/replay-occupied-tail.1"),
        &["C", "F", "JXG"],
        None,
    ),
    (
        include_str!("live_replay/replay-overwritten-tail.1"),
        &["C", "F", "XG"],
        None,
    ),
    (
        include_str!("live_replay/replay-marker-with-suffix.1"),
        &["FGD", " X"],
        None,
    ),
    (
        include_str!("live_replay/replay-blank-before-ink.1"),
        &[" D X"],
        None,
    ),
    (
        include_str!("live_replay/replay-two-spaces.1"),
        &["AFG", "FG  X"],
        None,
    ),
    (
        include_str!("live_replay/replay-marker-then-strong.1"),
        &["CD", "J"],
        None,
    ),
    (
        include_str!("live_replay/replay-marker-then-emphasis.1"),
        &["D", "J"],
        None,
    ),
    (
        include_str!("live_replay/replay-gap-link-X.1"),
        &["A  X"],
        Some("X"),
    ),
    (
        include_str!("live_replay/replay-pending-glyph-tail.1"),
        &["X", "D"],
        None,
    ),
];

#[test]
fn live_field_replay_preserves_passes_and_scalar_owners() {
    // CVS term_flushln() prints each accepted pass separately and skips
    // ordinary SP at the restart. encode1() can retain an earlier glyph
    // between the replayed separator and the new word's first graph.
    for &(source, rows, label) in LIVE_REPLAY_CASES {
        let original =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let mut content = original;
        for _ in 0..3 {
            assert_eq!(
                description_rows(&mant_render::render_query_man(&content), source),
                rows,
                "{source}"
            );
            let mut labels = Labels::default();
            labels.visit_document(content.document.as_ref().unwrap());
            assert_eq!(
                labels.0,
                label
                    .map(|label| ("https://f.test".into(), label.into()))
                    .into_iter()
                    .collect::<Vec<_>>()
            );
            for word in ["D", "J", "DJ", "F", "FX", "X", "XG", "G"] {
                let hits = mant_query::search_query(
                    &content,
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
                    u32::from(
                        rows.iter()
                            .any(|row| row.split_whitespace().any(|part| part == word))
                    ),
                    "{word}: {source}"
                );
            }
            let markdown = mant_codec::encode::render_markdown(&content);
            content =
                json_string_roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
        }
    }
}

#[test]
fn inset_body_writes_can_replace_an_open_head_cell() {
    // CVS termp_it_pre() calls term_word() for the generated inset
    // separator. encode1() does not replace an occupied cell with NBRSP;
    // the following BODY NBRZW then replaces the glyph at its real cursor.
    // The same source words without the list's separator retain J instead.
    for (source, row, label) in [
        (include_str!("live_replay/handoff-normal-zero.1"), "C", None),
        (
            include_str!("live_replay/handoff-before-link-zero.1"),
            "C",
            Some("C"),
        ),
        (
            include_str!("live_replay/handoff-without-z-zero.1"),
            "CJ",
            None,
        ),
        (include_str!("live_replay/replay-short-3.1"), "J", None),
    ] {
        let mut content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        for _ in 0..3 {
            let painted = mant_render::render_query_man(&content);
            let rows = description_rows(&painted, source);
            assert_eq!(rows.len(), 1, "{painted}");
            assert_eq!(
                rows[0].strip_prefix("• ").unwrap_or(rows[0]),
                row,
                "{source}"
            );
            let mut labels = Labels::default();
            labels.visit_document(content.document.as_ref().unwrap());
            assert_eq!(
                labels.0,
                label
                    .map(|label| ("https://c.test".into(), label.into()))
                    .into_iter()
                    .collect::<Vec<_>>()
            );
            let markdown = mant_codec::encode::render_markdown(&content);
            content = mant_loader::load_markdown_text(&markdown, None).unwrap();
        }
    }
}
