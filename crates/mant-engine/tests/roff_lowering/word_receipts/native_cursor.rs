//! Control-only operands can relocate, but do not close, a live field.

use super::{Labels, Visit, description_rows, json_string_roundtrip};
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};

const HEADER: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n";
const END: &str = "\n.Sh ENDTEST\nDone.\n";
type CursorCase = (
    &'static str,
    &'static [&'static str],
    &'static [(&'static str, u32)],
);

#[test]
fn control_only_rewinds_leave_the_existing_extent_editable() {
    // term_word() moves col back to write the marker; bufferc() leaves
    // lastcol beyond that write. The following run-in separator and NBRZW
    // can replace the old graph, or term_fill() can accept its suffix.
    let cases: &[CursorCase] = &[
        (
            // Pinned `mandoc -Tutf8` (`mdoc_term.c:764-766`, `chars.c:50`,
            // `term.c:630-632, 888-930`): `\ ` writes `encode1(0x00A0)` over
            // the `\b` after `\n` and `\&` overwrites `A`, so `term_field()`
            // emits one row containing U+00A0 (`""` once trimmed).
            r#".Bl -inset
.It Em "A" No "\h'-7n'\p"
.No "\&"
.El"#,
            &[""],
            &[("A", 0)],
        ),
        (
            r#".Sy "D \zJ" No "\h'-7n'\p""#,
            &["D", "J"],
            &[("D", 1), ("J", 1), ("DJ", 0)],
        ),
        (
            r#".Bl -inset
.It Em "\zC\&J" No "\h'-2n'\p"
.Pp
.No X
.El"#,
            &["", "", "X"],
            &[("C", 0), ("J", 0), ("X", 1)],
        ),
        (
            // The second '\p' still lies after the previous ink.
            // term_word() allows the following rewind to cover C.
            r#".Em "\h'-3n'B\pC\p" No "\h'-3n'Y""#,
            &["BY"],
            &[("BY", 1), ("C", 0)],
        ),
        (
            r#".Em "\h'-3n'B\pC\p" No "Y""#,
            &["BC", "Y"],
            &[("BC", 1), ("Y", 1), ("BCY", 0)],
        ),
        (
            r#".No "A\pB\p" Em "\h'1n'\h'-1n'\z\pC" Ns No "\zFG\h'2n'\p\h'2n'" Ns Sy "\h'1n'\h'-2n'\fPX""#,
            &["AB", "G   X"],
            &[("AB", 1), ("F", 0), ("C", 0)],
        ),
    ];
    for &(body, expected, words) in cases {
        let source = format!("{HEADER}{body}{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let rendered = mant_render::render_query_man(&content);
        assert_eq!(description_rows(&rendered, &source), expected, "{source}");
        for &(pattern, hits) in words {
            let result = mant_query::search_query(
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
            assert_eq!(result.total, hits, "{source}: {pattern}");
        }
    }
}

#[test]
fn a_control_only_prefix_remains_editable_until_its_accepted_rows() {
    // term_word() writes both \& and ignored device escapes as NBRZW
    // (term.c:799). term_fill() counts that zero-width slot as graph before
    // it decides whether the following source space consumes a pending \p.
    // A relocated X can replace the slot, so the native endline belongs
    // after the actual printed X, not its neighbor's first empty fragment.
    let cases: &[(&str, &[&str], bool)] = &[
        (r"\X'foo'\p ", &["X Y"], false),
        (
            r"\X'tty: link https://gap.test'\p \X'tty: link'",
            &["XY"],
            true,
        ),
        (r"\p\X'foo' ", &["X", "Y"], false),
        (r"\&\p ", &["X Y"], false),
    ];
    for &(prefix, expected, has_link) in cases {
        // Without the rewind, NBRZW still establishes the preceding empty
        // pass. Its endline must not split the following accepted YX row.
        for (edit, rows) in [(r"\h'-3n'", expected), ("", &["", "YX"][..])] {
            let source = format!("{HEADER}.No \"{prefix}\" Ns No \"Y{edit}\" Ns No X{END}");
            let content =
                json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
            let rendered = mant_render::render_query_man(&content);
            assert_eq!(description_rows(&rendered, &source), rows, "{source}");
            let mut labels = Labels::default();
            labels.visit_document(content.document.as_ref().unwrap());
            let links = has_link
                .then(|| ("https://gap.test".to_owned(), String::new()))
                .into_iter()
                .collect::<Vec<_>>();
            assert_eq!(labels.0, links, "{source}");
        }
    }
}

#[test]
fn zero_relative_motion_preserves_raw_word_slots_for_a_later_edit() {
    // term_word() handles the BACKAFTER flag before measuring HORIZ.
    // A parsed zero relative length then writes no native slots, even
    // when it appears inside or at the end of the preceding word.
    for word in [
        "BC",
        r"\h'0n'BC",
        r"B\h'0n'C",
        r"BC\h'0n'",
        r"\h'-0n'BC",
        r"\h'0m'BC",
        r"\z\h'0n'BC",
        r"B\zC\h'0n'",
        r"\h'|0n'BC",
    ] {
        let source = format!("{HEADER}.No \"{word}\" Ns No \"\\h'-3n'Y\"{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let rendered = mant_render::render_query_man(&content);
        assert_eq!(description_rows(&rendered, &source), ["YC"], "{source}");
    }
}

#[test]
fn a_bounded_relative_retreat_keeps_unread_markers_and_later_ink() {
    // term.c::term_word() retreats through native bytes while retaining
    // lastcol. The next operand can replace or overprint a graph before
    // a marker that term_fill() has yet to consume.
    let cases = [
        (
            r#".No A No "\z\h'-1n'\h'1n'" No "F\pG\p\c\h'-4n'" No "\h'3n'\h'-4n'X""#,
            "A   XG",
        ),
        (
            r#".Sy A No "\z\h'-1n'\h'1n'" Sy "F\pG\p\c\h'-5n'" Em "\h'3n'\h'-4n'X""#,
            "A   FXG",
        ),
        (
            r#".No A No "\h'1n'" No "F\pG\p\c" No "\h'3n'\h'-4n'X""#,
            "A   FGX",
        ),
    ];
    for (body, expected) in cases {
        let source = format!("{HEADER}{body}{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let text = mant_render::render_query_man(&content);
        assert_eq!(description_rows(&text, &source), [expected], "{source}");
    }
}

#[test]
fn a_trailing_native_advance_is_only_printed_before_later_ink() {
    // term_word() writes each HORIZ advance. term_field() prints those
    // columns only if a later printable cell remains in the same pass.
    // A replay can revisit those columns without adding a second gap.
    let cases = [
        (r#".No "A\h'2n'" No X"#, "A   X"),
        (r#".No "A\h'2n'" Ns No X"#, "A  X"),
        (r#".No "A\h'2n'" Ns No "\h'-1n'X""#, "A X"),
        (r#".No "A\h'2n'""#, "A"),
        (
            r#".No "A\h'2n'" No "\z\h'-1n'\h'1n'" Ns No " F\pG\p\c" No "\h'-4n'X""#,
            "A     XG",
        ),
        (
            r#".Em "A\h'2n'" Em "\z\h'-1n'\h'1n'" Ns No " F\pG\p\c" Sy "\h'-4n'X""#,
            "A     X",
        ),
        (
            r#".Em "A\h'2n'" Em "\z\h'-1n'\h'1n'" Ns No " F\pG\p\c" No "\h'-4n'X""#,
            "A     XG",
        ),
        (
            r#".Em "A\h'2n'" Em "\h'1n'" Ns No " F\pG\p\c" Sy "\h'-4n'X""#,
            "A     X",
        ),
        (
            r#".Em "A\h'1n'" Em "\z\h'-1n'\h'1n'" Ns No " F\pG\p\c" Sy "\h'-4n'X""#,
            "A    X",
        ),
    ];
    for (body, expected) in cases {
        let source = format!("{HEADER}{body}{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let text = mant_render::render_query_man(&content);
        assert_eq!(description_rows(&text, &source), [expected], "{source}");
    }
}

#[test]
fn a_fresh_retreat_keeps_inner_and_trailing_native_advances_editable() {
    // term_word() changes offset for a retreat before col zero. Both
    // positive advances still write into the new field. A later tight
    // HORIZ can consume either extent and overwrite C or precede it.
    let cases = [
        (r#".No "\h'-3n'B\h'1n'C\h'1n'" Ns No "\h'-3n'Y""#, "BYC"),
        (r#".No "\h'-3n'B\h'1n'C\h'1n'" Ns No "\h'-6n'Y""#, "Y C"),
        (r#".No "\h'-3n'B\h'2n'C\h'2n'" Ns Em "\h'-3n'Y""#, "B  Y"),
        (r#".No "\h'-3n'B\h'2n'C\h'2n'" Ns No "\h'-3n'Y""#, "B  Y"),
        (r#".No "B\h'2n'C\h'2n'" Ns No "\h'-3n'Y""#, "B  Y"),
        (r#".No "\h'-1n'B\h'2n'C\h'1n'" No "\h'-3n'Y""#, "B  Y"),
    ];
    for (body, expected) in cases {
        let source = format!("{HEADER}{body}{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let text = mant_render::render_query_man(&content);
        assert_eq!(description_rows(&text, &source), [expected], "{source}");
    }
}

#[test]
fn trailing_authored_blanks_keep_decorated_native_words_live() {
    // term.c::encode() writes trailing SP without committing the earlier
    // font cells. The later HORIZ can replace a glyph with NBRZW and
    // leave just its underline on the first accepted row.
    for (font, ink, next, columns, expected) in [
        ("Sy", " DJ ", "Sy", 8, &["", "F"][..]),
        ("Sy", " DJ ", "No", 8, &["", "FJ"]),
        ("Sy", " DJ ", "Sy", 6, &[" D", "F"]),
        ("Sy", "DJ ", "No", 3, &["DJ", "F"]),
        ("No", " DJ ", "Sy", 8, &["", "F"]),
        ("Em", " DJ ", "No", 6, &[" _", "F"]),
        ("Em", "DJ ", "Sy", 3, &["D_", "F"]),
    ] {
        let source = format!(r#"{HEADER}.{font} "{ink}" {next} "\h'-{columns}n'\&\p\:F\p"{END}"#);
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let text = mant_render::render_query_man(&content);
        assert_eq!(description_rows(&text, &source), expected, "{source}");
    }
}

#[test]
fn a_complete_absolute_cursor_leaves_later_slots_available_to_replay() {
    // term.c::term_word() moves col to the absolute target while
    // bufferc() keeps lastcol. The next word can write at the early
    // target, while term_fill() still accepts the original suffix.
    let cases = [
        (
            include_str!("live_replay/replay-keeps-absolute-cursor-before-ink.1"),
            "X   FG",
            "X",
        ),
        (
            include_str!("live_replay/replay-covers-ink-after-absolute-cursor.1"),
            "A   FG",
            "A",
        ),
    ];
    for (source, expected, retained) in cases {
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let rendered = mant_render::render_query_man(&content);
        assert_eq!(description_rows(&rendered, source), [expected], "{source}");
        for pattern in ["A", "X", "FG"] {
            let result = mant_query::search_query(
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
            assert_eq!(
                result.total,
                u32::from(pattern == retained || pattern == "FG")
            );
        }
    }
}

#[test]
fn backafter_cancels_an_absolute_target_after_a_relative_rewind() {
    // term.c::term_word() clears BACKAFTER before HORIZ reads the
    // target. X replaces the invisible slot reached by the relative
    // rewind, and the original ABC remains in the accepted field.
    let words = r#".No "ABC\&\h'-1n'\z\h'|1n'\c" No X"#;
    let source = format!("{HEADER}{words}{END}");
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let rendered = mant_render::render_query_man(&content);
    assert_eq!(description_rows(&rendered, &source), ["ABCX"], "{source}");
}

#[test]
fn absolute_motion_can_revisit_ink_written_into_a_fresh_field() {
    // term.c::term_word() resolves absolute HORIZ from the field's
    // actual col. With no earlier cells the first operand owns the
    // full field, even when a word-end marker still awaits term_fill().
    let source = include_str!("live_replay/replay-keeps-absolute-cursor-in-fresh-field.1");
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    let rendered = mant_render::render_query_man(&content);
    let rows: Vec<_> = description_rows(&rendered, source)
        .into_iter()
        .map(str::trim_start)
        .collect();
    assert_eq!(
        rows,
        [
            "Fresh zero cursor",
            "XB",
            "",
            "Fresh interior cursor",
            "ABX",
            "",
            "Cursor at end",
            "AB X",
            "",
            "Existing proven prefix",
            "XB",
            "",
            "Literal field",
            "XB",
            "",
            "Absolute canceled by armed escape",
            "AX",
            "",
            "Marker stays in the native field",
            "XB",
        ]
    );
}

#[test]
fn overwritten_underline_control_releases_the_surviving_ink() {
    // encode1() writes '_' then BS for emphasis. A later HORIZ
    // can replace BS without replacing '_', so term_field() emits the
    // underscore at its own column. An intact pair remains a style.
    const PAGE: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n.Ss CASE0CASE\n";
    let mut cases = Vec::new();
    for first in ["No", "Em", "Sy"] {
        for (last, expected) in [("No", "A   F_XG"), ("Em", "A   F_X")] {
            cases.push((format!(
                r#".{first} "A" No "\z\h'-1n'\h'1n'" Em "F\pG\p\c\h'-3n'" {last} "\h'3n'\h'-4n'X""#
            ), expected));
        }
    }
    cases.extend([
        (r#".Sy "AB" No " \h'1n'\h'-6n'\z\pC" Ns Em "\zFG\h'2n'\h'-4n'\p\0" Ns Sy "\h'1n'\h'-2n'\fPX""#.into(), "A_X"),
        // The UTF-8 device writes \0 through encode1(), unlike the ASCII sentinel.
        (r#".Sy "AB" No " \h'1n'\h'-6n'\z\pC" Ns Em "\zFG\h'2n'\p\0\z" Ns Sy "\h'1n'\h'-2n'\fPX""#.into(), "AG  _X"),
        (r#".Sy "AB" Em " \h'1n'\h'-6n'\z\pC " Em "\zFG\h'2n'\p\0\z" Ns Sy "\h'1n'\h'-2n'\fPX\p""#.into(), "AC\nG  _X"),
        (r#".Em "F" Em "G""#.into(), "F G"),
        (r#".Sy "_" No "X""#.into(), "_ X"),
    ]);
    for (words, expected) in cases {
        let source = format!("{PAGE}{words}\n.Ss CASE999CASE\nDone.\n");
        let document =
            super::json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let markdown = mant_codec::encode::render_markdown(&document);
        let readback = super::json_string_roundtrip(
            &mant_loader::load_markdown_text(&markdown, None).unwrap(),
        );
        for output in [
            mant_render::render_query_man(&document),
            mant_render::render_query_text(&readback),
        ] {
            let (_, body) = output.split_once("CASE0CASE\n").unwrap();
            let (body, _) = body.split_once("CASE999CASE\n").unwrap();
            assert_eq!(
                body.trim().lines().map(str::trim).collect::<Vec<_>>(),
                expected.lines().collect::<Vec<_>>(),
                "{source}\n{output}"
            );
        }
    }
}
