//! Accepted backspaces can revisit a blank column before their field.

use super::{description_rows, json_string_roundtrip};
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};

const HEADER: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";
const WORDS: &str = r#".Sy "\zC\zJ" No "\h'-3n'\&""#;
const END: &str = "\n.Sh ENDTEST\nDone.\n";

#[test]
fn section_and_subsection_headings_use_their_native_origin() {
    // mdoc_term.c::termp_ss_pre() positions HEAD at half the default
    // indent plus one basic unit. Sh starts at the page edge.
    let (meta, _) = HEADER.split_once(".Sh DESCRIPTION\n").unwrap();
    for (section, macro_name, expected) in [("", "Sh", "J"), (".Sh DESCRIPTION\n", "Ss", "JC")] {
        let source = format!("{meta}{section}.{macro_name} \"\\zC\\zJ\\h'-2n'\\&\"\n.No body{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let rendered = mant_render::render_query_man(&content);
        let heading = rendered
            .lines()
            .map(str::trim)
            .find(|line| matches!(*line, "J" | "JC" | "CJ"));
        assert_eq!(heading, Some(expected), "{source}");
    }
}

#[test]
fn accepted_backspaces_use_the_actual_left_margin() {
    // term.c::term_field() skips NBRZW before emitting a backspace.
    // With one usable column before the field, J lands to the left
    // of C; at the page edge it covers C. Both pinned devices agree.
    for (offset, expected) in [
        (None, "JC"),
        (Some("-4n"), "JC"),
        (Some("-5n"), "J"),
        (Some("-6n"), "J"),
        (Some("indent"), "JC"),
    ] {
        let body = offset.map_or_else(
            || WORDS.to_owned(),
            |offset| format!(".Bd -ragged -offset {offset}\n{WORDS}\n.Ed"),
        );
        let source = format!("{HEADER}{body}{END}");
        let mut content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        for _ in 0..3 {
            let rendered = mant_render::render_query_man(&content);
            let rows = description_rows(&rendered, &source).join("\n");
            assert_eq!(rows.trim(), expected, "{source}");
            for pattern in ["CJ", "JC", "J", "C"] {
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
                assert_eq!(hits.total, u32::from(pattern == expected), "{source}");
            }
            let markdown = mant_codec::encode::render_markdown(&content);
            content =
                json_string_roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
        }
    }
}

#[test]
fn edited_font_copies_share_their_original_style_and_each_keep_their_own_column() {
    // term.c::encode1() writes two copies around a backspace for bold
    // text. Overwriting that backspace with NBRZW keeps both copies in
    // the accepted buffer. term_field() skips NBRZW, so each copy has
    // its own visible column, whereas the untouched bold pair has one.
    // UTF-8 width-one operands can also complete BACKBEFORE within a word
    // before a later operand overwrites the native backspace (term.c:903).
    for (words, expected) in [
        (r#".Sy "C\zJ" No "\h'-5n'\&""#, "CCJ"),
        (r#".Sy "C\zJ" No "\h'-2n'\&""#, "CJJ"),
        (r#".Sy "é\zJ" No "\h'-5n'\&""#, "ééJ"),
        (r#".Sy "C\zé" No "\h'-2n'\&""#, "Céé"),
        (r#".Sy "é\zJX" No "\h'-5n'\&""#, "éJX"),
        (r#".Sy "é\zJX""#, "éX"),
        (r#".Sy "C\zéX" No "\h'-2n'\&""#, "CX"),
        (".Sy CJ", "CJ"),
    ] {
        let source = format!("{HEADER}{words}{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let markdown = mant_codec::encode::render_markdown(&content);
        assert!(markdown.contains(&format!("**{expected}**")), "{markdown}");
        let readback =
            json_string_roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
        for document in [content, readback] {
            let text = mant_render::render_query_man(&document);
            assert_eq!(
                description_rows(&text, &source).join("\n").trim(),
                expected,
                "{source}"
            );
            let found = mant_query::search_query(
                &document,
                &SearchQuery {
                    pattern: expected.to_owned(),
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
            assert_eq!(found.total, 1, "{source}");
        }
    }
}

#[test]
fn edited_font_copies_keep_unchanged_whitespace_and_styling() {
    // term.c::encode1() emits a bold glyph on either side of BS.
    // After a marker overwrites BS, term_field() skips that marker
    // and the two copies print at distinct device columns.
    for (source, expected) in [
        (
            include_str!("live_replay/replay-keeps-repeated-font-glyph-after-blank.1"),
            "D JJ",
        ),
        (
            include_str!("live_replay/replay-keeps-repeated-font-glyph-before-stable-suffix.1"),
            "D KKJ",
        ),
        (
            include_str!("live_replay/replay-keeps-repeated-font-glyph-before-trailing-blanks.1"),
            "C JJ",
        ),
        (
            include_str!("live_replay/replay-keeps-separate-repeated-font-glyphs.1"),
            "D KK LRR",
        ),
        (
            include_str!("live_replay/replay-keeps-repeated-font-glyph-after-cancelled-arm.1"),
            "CJJ",
        ),
        (
            include_str!("live_replay/replay-keeps-repeated-font-glyph-after-nbsp-arm.1"),
            "DD",
        ),
    ] {
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let markdown = mant_codec::encode::render_markdown(&content);
        assert!(markdown.contains(&format!("**{expected}**")), "{markdown}");
        let readback =
            json_string_roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
        for document in [content, readback] {
            let rendered = mant_render::render_query_man(&document);
            let (_, body) = rendered.split_once("CASE0CASE\n").unwrap();
            let (body, _) = body.split_once("CASE999CASE\n").unwrap();
            assert_eq!(body.trim(), expected, "{source}");
        }
    }
}

#[test]
fn canceling_an_arm_does_not_publish_a_previously_rejected_buffer() {
    // term.c::term_word() cancels the armed glyph locally. This does not
    // bypass term_fill() when the rest of the control word rejects its
    // prefix and suffix together.
    let source = include_str!("live_replay/replay-keeps-rejection-after-cancelled-arm.1");
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let rendered = mant_render::render_query_man(&content);
    let (_, body) = rendered.split_once("CASE0CASE\n").unwrap();
    let (body, _) = body.split_once("CASE999CASE\n").unwrap();
    assert!(body.trim().is_empty(), "{rendered}");
}

#[test]
fn suffix_replay_preserves_earlier_accepted_rows_with_covered_glyphs() {
    // term.c::term_field() releases padding ahead of a backspace. The
    // following bold G still covers F in that same column. A later edit
    // to the field suffix cannot detach the already proved earlier slots.
    let words = r#".Sy "AB" No "\h'3n'\h'-6n'\z\p\:C" Sy "\zFG\h'2n'\p\~" Ns Sy "\h'1n'\h'-2n'\fPX\h'-4n'""#;
    let source = format!("{HEADER}{words}{END}");
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let text = mant_render::render_query_man(&content);
    assert_eq!(
        description_rows(&text, &source),
        ["AB", "CG  X"],
        "{source}"
    );
}

#[test]
fn accepted_rewind_can_cover_ink_after_a_nonbreaking_blank() {
    // term.c::encode1() decorates U+00A0 on the UTF-8 device. The
    // overprint keeps its blank column while J covers C to its right.
    let words = r#".Sy "\~C\zJ" No "\h'-3n'\&""#;
    let source = format!("{HEADER}{words}{END}");
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let text = mant_render::render_query_man(&content);
    assert_eq!(description_rows(&text, &source), ["\u{a0}J"], "{source}");
}

#[test]
fn replay_keeps_accepted_columns_of_compact_motion_runs() {
    // term.c::term_word() can revisit native advances which have no
    // reading scalar. term_field() still emits their columns alongside
    // the surviving ink, including a gap that covers an earlier glyph.
    for (words, expected) in [
        (
            r#".Em "A\zC\fPD" No "\h'-1n'\z\h'1n'F\fRG\h'-1n'" No "\h'3n'F\zG\h'2n'\:" No "\p\h'-1n'\fPX""#,
            "ADFG   FG  X",
        ),
        (
            r#".No "A\zC\fPD" Em "\:\h'-3n'\:" No "\h'3n'D  " Em "\h'-1n'\fPX""#,
            "A   D  X",
        ),
    ] {
        let source = format!("{HEADER}{words}{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let text = mant_render::render_query_man(&content);
        assert_eq!(description_rows(&text, &source), [expected], "{source}");
    }
}

#[test]
fn replayed_motion_after_a_word_end_keeps_the_preceding_glyph_and_row() {
    // term.c::term_fill() consumes the `\p` at a word boundary. Spaces
    // on the next row cannot cover C after the first pass backspaces.
    let words = r#".No "A\zC\fPD" Em "\:\h'-3n'\p\:" No "\h'3n'D  " Em "\h'-1n'\fPX""#;
    let source = format!("{HEADER}{words}{END}");
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let text = mant_render::render_query_man(&content);
    assert_eq!(description_rows(&text, &source), ["AC", "D  X"], "{source}");
}

#[test]
fn replay_preserves_a_single_advance_before_retained_ink() {
    // term.c::term_field() releases the spaces before the first ink.
    // Replaying that advance can retain earlier K, replace K with X,
    // or place X on the other side of the advance.
    for (advance, tail, expected) in [
        ("", r#"Ns No "F\pG\0\c" No "\h'-4n'X""#, "A   KXG\u{a0}"),
        ("", r#"Ns No "F\pG\~\p\c" No "\h'-4n'X""#, "A   KFXG\u{a0}"),
        ("", r#"Ns No "F\c" Em "\h'-4n'X""#, "A   XF"),
        ("", r#"Ns No "F\pG\&\c" No "\h'-9n'X""#, "A X KFG"),
        ("", r#"No "F\pG\p\c" No "\h'-4n'X""#, "A   K XG"),
        (r"\h'1n'", r#"Ns No "F\pG\p\c" No "\h'-4n'X""#, "A    KXG"),
    ] {
        let source = format!("{HEADER}.Sy \"A\" Em \"\\z\\h'-1n'\\h'1n'{advance} K\" {tail}{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let text = mant_render::render_query_man(&content);
        assert_eq!(description_rows(&text, &source), [expected], "{source}");
    }
}

#[test]
fn covered_local_glyphs_release_the_full_following_advance() {
    // term.c::encode1() covers the first glyph with the second and clears
    // BACKBEFORE. Only a still-pending glyph consumes the first advance cell.
    let source = include_str!("live_replay/replay-keeps-advance-after-covered-glyph.1");
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let text = mant_render::render_query_man(&content);
    let lines: Vec<_> = description_rows(&text, source)
        .into_iter()
        .map(str::trim_start)
        .collect();
    assert_eq!(
        lines,
        [
            "Covered before word-end marker",
            "G  Z",
            "",
            "Covered without marker",
            "G  Z",
            "",
            "Ordinary advance",
            "FG  Z",
            "",
            "Pending glyph",
            "F Z",
            "",
            "Underlined ink",
            "G  Z",
            "",
            "Bold ink",
            "G  Z",
            "",
            "Following glyph rewritten",
            "G  W",
            "",
            "Later absolute cursor",
            "G X",
        ]
    );
}

#[test]
fn inherited_glyphs_release_only_the_advance_cells_cvs_writes() {
    // term.c::encode1() handles inherited BACKBEFORE before the local
    // BACKAFTER can hold another glyph. ESCAPE_HORIZ spends its first
    // unit on a still-held glyph and records only the remaining cells.
    let source = include_str!("live_replay/replay-keeps-advances-after-inherited-glyph.1");
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let text = mant_render::render_query_man(&content);
    let lines: Vec<_> = description_rows(&text, source)
        .into_iter()
        .map(str::trim_start)
        .collect();
    assert_eq!(
        lines,
        [
            "Plain inherited glyph",
            "F  G      X",
            "",
            "Bold prefix",
            "ABF  G      X",
            "",
            "Italic advance with bold prefix",
            "ABF  G      X",
            "",
            "Native breakpoint with bold glyph",
            "AF G    X",
            "",
            "Covered local glyph before marker",
            "ABG  K X",
            "",
            "Covered local glyph before later ink",
            "ABG  Z X",
            "",
            "Shorter relative advance",
            "ABF G     X",
            "",
            "Advance consumed by the pending glyph",
            "FG",
            "",
            "Advance before new ink",
            "F  G",
            "",
            "Full advance after covering both glyphs",
            "G  Z",
            "",
            "Inherited glyph without another arm",
            "F   G",
            "",
            "Unused trailing advance",
            "F  G",
        ]
    );
}

#[test]
fn a_final_absolute_cursor_does_not_erase_an_established_advance() {
    // term.c::term_word() changes col without truncating lastcol. The
    // earlier advance remains between the same accepted ink even when
    // the last absolute target moves before, into, or beyond that word.
    for (words, expected) in [
        (r#".No "\zFG\h'2n'X\h'|0n'""#, "G  X"),
        (r#".No "\zFG\h'2n'X\h'|4n'""#, "G  X"),
        (r#".No "\zFG\h'2n'X\h'|8n'""#, "G  X"),
        (r#".No "\zF\h'2n'X\h'|0n'""#, "F X"),
        (r#".No "\zFG\p\h'2n'X\h'|0n'""#, "G  X"),
    ] {
        let source = format!("{HEADER}{words}{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let text = mant_render::render_query_man(&content);
        assert_eq!(description_rows(&text, &source), [expected], "{source}");
    }
}

#[test]
fn relative_rewind_preserves_the_cursor_inside_an_unread_advance() {
    // term_word() keeps lastcol while HORIZ rewinds col. The next
    // term_word() overwrites the retained fixed cells before term_fill()
    // decides whether the earlier marker causes an endline.
    for (words, expected) in [
        (
            r#".Em "\h'-1n'F\p\h'2n'\h'-2n'" Ns Sy "\~" Sy "\h'-2n'X""#,
            "FX",
        ),
        (
            r#".Em "\h'-1n'F\p\h'2n'\h'-2n'" Ns Sy "\~" No "\h'-2n'X""#,
            "FX",
        ),
        (
            r#".Em "\h'-1n'F\p\h'2n'\h'-1n'" Ns Sy "\~" Sy "\h'-2n'X""#,
            "F X",
        ),
        (
            r#".Em "\h'-1n'F\p\h'2n'\h'0n'" Ns Sy "\~" Sy "\h'-2n'X""#,
            "F  X",
        ),
        (
            r#".Em "\h'-1n'F\h'2n'\h'-2n'" Ns Sy "\~" Sy "\h'-2n'X""#,
            "FX",
        ),
        (
            r#".Em "\h'-1n'F\p\h'2n'\h'-2n'" Ns Sy "B" Sy "\h'-2n'X""#,
            "FX",
        ),
        (
            r#".Em "\h'-1n'F\p\h'2n'\h'-2n'" Ns Sy "\&" Sy "\h'-2n'X""#,
            "FX",
        ),
        (
            r#".Em "\h'-1n'F\p\h'3n'\h'-2n'" Ns Sy "\~" Sy "\h'-2n'X""#,
            "F X",
        ),
        (
            r#".Em "\h'-1n'F\p\h'3n'\h'-3n'" Ns Sy "\~" Sy "\h'-2n'X""#,
            "FX",
        ),
    ] {
        let source = format!("{HEADER}{words}{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let rendered = mant_render::render_query_man(&content);
        assert_eq!(description_rows(&rendered, &source), [expected], "{source}");
    }
}
