//! The accepted device row decides the surviving text and annotation owner.

use super::{Labels, Visit, description_rows, json_string_roundtrip};
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};

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
