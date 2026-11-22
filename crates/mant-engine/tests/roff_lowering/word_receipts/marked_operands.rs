//! Native marker writes stay editable across source operands.

use super::json_string_roundtrip;

const HEADER: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n.Ss CASE0CASE\n";
const END: &str = ".Ss CASE999CASE\nDone.\n";

#[test]
fn leading_advances_in_definition_lines_stay_editable_until_flush() {
    // term.c::ESCAPE_HORIZ writes a native NBRSP before C. The following
    // word can overwrite C or J before termp_it_post() closes the field.
    // Pp and the literal display independently close the active row.
    type Case = (
        &'static str,
        &'static str,
        i8,
        &'static str,
        &'static [&'static str],
    );
    let cases: &[Case] = &[
        ("-inset", "1n", -2, ".Pp\n.No X", &["C", "", "X"]),
        (
            "-inset",
            "1n",
            -2,
            ".Bd -literal\n.No X\n.Ed",
            &["C", "", "X"],
        ),
        ("-inset", "1n", -4, ".No X", &["X"]),
        ("-inset", "1n", -4, ".Pp\n.No X", &["J", "", "X"]),
        ("-ohang", "2n", -2, ".No X", &["C", "X"]),
        (
            "-ohang",
            "2n",
            -4,
            ".Bd -literal\n.No X\n.Ed",
            &["J", "", "X"],
        ),
        ("-ohang", "0n", -2, ".No X", &["C", "X"]),
        ("-ohang", "1n", -1, ".No X", &["CJ", "X"]),
    ];
    for &(kind, advance, retreat, body, expected) in cases {
        let source = format!(
            "{HEADER}.Bl {kind}\n.It No \"\\h'{advance}'C\\pJ\" No \"\\h'{retreat}n'\\p\"\n{body}\n.El\n{END}"
        );
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let rendered = mant_render::render_query_man(&content);
        let (_, body) = rendered.split_once("CASE0CASE\n").unwrap();
        let (body, _) = body.split_once("CASE999CASE\n").unwrap();
        let rows: Vec<_> = body.trim().lines().map(str::trim).collect();
        assert_eq!(rows, expected, "{source}\n{rendered}");
    }
}

#[test]
fn accepted_leading_motion_keeps_its_place_on_shared_and_fresh_rows() {
    // term.c::term_field() holds HORIZ and the initial vbl together until
    // a real glyph prints. The first accepted graph owns an unprojected
    // advance within its interval. The independent Pp device rows retain
    // their own origins, and a later X must not claim an earlier separator.
    #[derive(Clone, Copy)]
    enum Placement {
        Shared,
        WidthLimited,
        ExpandedHead,
        Fresh,
    }
    use Placement::{ExpandedHead, Fresh, Shared, WidthLimited};

    const FIXED: &str = r"\h'1n'C\pJ";
    const ZERO: &str = r"\h'0n'C\pJ";
    let shared = &["TERM   C", "      X"][..];
    let cases = [
        ("hang", 4, Shared, FIXED, -2, shared),
        ("hang", 4, WidthLimited, FIXED, -2, shared),
        ("tag", 4, Shared, FIXED, -2, shared),
        ("tag", 4, ExpandedHead, FIXED, -2, shared),
        (
            "tag",
            4,
            ExpandedHead,
            FIXED,
            -4,
            &["TERM   J", "      X"][..],
        ),
        ("tag", 4, WidthLimited, FIXED, -2, shared),
        ("hang", 0, Shared, FIXED, -4, &["TERM  X"][..]),
        (
            "hang",
            0,
            Fresh,
            FIXED,
            -2,
            &["TERM", "", "   C", "", "  X"][..],
        ),
        (
            "hang",
            0,
            Fresh,
            ZERO,
            -2,
            &["TERM", "", "  C", "", "  X"][..],
        ),
        ("hang", 4, Shared, ZERO, -2, &["TERM  C", "      X"][..]),
        (
            "hang",
            4,
            Shared,
            r"\h'1n'\p\&",
            -2,
            &["TERM", "      X"][..],
        ),
        (
            "hang",
            4,
            Fresh,
            FIXED,
            -2,
            &["TERM", "", "       C", "", "      X"][..],
        ),
        (
            "hang",
            4,
            Fresh,
            ZERO,
            -2,
            &["TERM", "", "      C", "", "      X"][..],
        ),
        (
            "hang",
            4,
            Fresh,
            FIXED,
            -4,
            &["TERM", "", "       J", "", "      X"][..],
        ),
    ];
    for (kind, width, placement, operand, retreat, expected) in cases {
        let (before, after) = match placement {
            Shared => (".It TERM\n", ""),
            WidthLimited => (".It TERM\n.ll 12n\n", ".ll\n"),
            ExpandedHead => (".It Xo TERM\n.br\n", ".Xc\n"),
            Fresh => (".It TERM\n.Pp\n", ".Pp\n"),
        };
        let source = format!(
            r#"{HEADER}.Bl -{kind} -width {width}n
{before}.No "{operand}" No "\h'{retreat}n'\p"
{after}.No X
.El
{END}"#
        );
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let rendered = mant_render::render_query_man(&content);
        assert_eq!(
            rows_relative_to_term_origin(&rendered),
            expected,
            "{source}\n{rendered}"
        );
    }
}

fn rows_relative_to_term_origin(rendered: &str) -> Vec<&str> {
    let (_, body) = rendered.split_once("CASE0CASE\n").unwrap();
    let (body, _) = body.split_once("CASE999CASE\n").unwrap();
    let shared_origin = " ".repeat(body.find("TERM").unwrap());
    body.trim_end()
        .lines()
        .map(|line| line.strip_prefix(&shared_origin).unwrap_or(line).trim_end())
        .collect()
}

#[test]
fn continued_literal_definition_retires_graphless_motion_padding() {
    // The continued source line has not called term_newln() yet
    // (mdoc_term.c::print_mdoc_node). The HORIZ and marker suffix remains
    // editable, then term_field() drops its trailing positioning after
    // printing START. An independent intentional empty row still lives.
    for (kind, width, origin) in [("hang", 0, 2), ("hang", 4, 6), ("tag", 0, 2), ("tag", 4, 6)] {
        let indent = " ".repeat(origin);
        let start = format!("{indent}START");
        let end = format!("{indent}X");
        let following = format!("{indent}Y");
        for (continued, motion, controls, tail, expected) in [
            (
                r"\c",
                1,
                r"\p\&",
                r#"No "\h'-2n'\p""#,
                &["", "TERM", &start, &end][..],
            ),
            (
                r"\c",
                2,
                r"\&\p",
                r#"No "\h'-4n'\p""#,
                &["", "TERM", &start, &end][..],
            ),
            (
                r"\c",
                1,
                r"\p\&",
                "No Y",
                &["", "TERM", &start, &following, &end][..],
            ),
            (
                "",
                1,
                r"\p\&",
                r#"No "\h'-2n'\p""#,
                &["", "TERM", &start, "", &end][..],
            ),
        ] {
            let source = format!(
                r#"{HEADER}.nf
.Bl -{kind} -width {width}n
.It TERM
.No START{continued}
.No "\h'{motion}n'{controls}" {tail}
.No X
.El
.fi
{END}"#,
            );
            let content =
                json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
            let rendered = mant_render::render_query_man(&content);
            let (_, body) = rendered.split_once("CASE0CASE\n").unwrap();
            let (body, _) = body.split_once("CASE999CASE\n").unwrap();
            let label = body
                .lines()
                .find(|line| line.trim_start() == "TERM")
                .unwrap();
            let prefix = label.strip_suffix("TERM").unwrap();
            let rows: Vec<_> = body
                .strip_suffix(prefix)
                .unwrap_or(body)
                .trim_end_matches('\n')
                .lines()
                .map(|line| line.strip_prefix(prefix).unwrap_or(line))
                .collect();
            assert_eq!(rows, expected, "{source}\n{rendered}");
        }
    }
}

#[test]
fn marked_operands_keep_raw_font_slots_and_accepted_rows() {
    // term.c::encode1() installs decoration bytes before a graph;
    // term_word() buffers SP and \p without collapsing them. The next
    // relative HORIZ edits those raw positions before term_fill().
    // The same field can remain editable across a control-only operand:
    // ESCAPE_NOSPACE cancels BACKAFTER before a later HORIZ consumes
    // BACKBEFORE. Keep the accepted glyphs and the exact interior rows.
    let cases: &[(&str, &[&str])] = &[
        (r#".Sy " D\pJ\p" Em "\h'-8n'\&\p\:F\p""#, &["D", "F"]),
        (r#".Sy " D\pJ\p" Sy "\h'-8n'\&\p\:F\p""#, &["D", "F"]),
        (r#".Em " D\pJ\p" Sy "\h'-5n'\&\p\:F\p""#, &["D", "F"]),
        (
            r#".Sy "C\p DJ" No "\h'-9n'\p\:F\p" No "\c\p\~" No "\h'-2n'X""#,
            &["C", "FDXJ"],
        ),
        (
            r#".No "C\p DJ" No "\h'-5n'\p\:F\p" No "\c\p\~" No "\h'-2n'X""#,
            &["C", "F", "X"],
        ),
        (r#".Sy " D\pJ\p" Em "\&\p\:F\p""#, &["DJ", "", "F"]),
        (r#".Sy " D\pJ" Em "\h'-7n'\&\p\:F\p""#, &["D", "F"]),
        (r#".No "C\p DJ" No "\&Z""#, &["C", "DJ Z"]),
        (
            r#".No "C\p DJ" No "\h'-5n'\p\:F\p" No "\c\p\~" No "\h'-2n'\&""#,
            &["C", "F"],
        ),
        (r#".Sy " D\pD\p" Em "\h'-8n'\&\p\:D\p""#, &["D", "D"]),
        (
            r#".Sy "C\p DJ" No "\h'-5n'\p\:F\p\h'2n'" Em "\c\p\~" No "\&\h'-2n'X""#,
            &["C", "D", "F", "X"],
        ),
        (
            r#".Sy "C\p DJ" Ns Sy "\h'-5n'\p\z\:F\p\z" Ns No " \c\h'2n'\p\h'-1n'\~" No "\h'-2n'X""#,
            &["C", "D", "F", "X"],
        ),
        (
            r#".Sy "C\p DJ" Em "\h'-5n'\p\:F\p\z" Ns No "\c\h'1n'\p\~" No "\h'-2n'X\h'1n'""#,
            &["C", "D", "F X"],
        ),
        (
            r#".No "A\pB\p" Em "\h'1n'\h'-1n'\z\pC" Ns No "\zFG\h'2n'\p\h'2n'" Ns Sy "\h'1n'\h'-2n'\fPX""#,
            &["AB", "G   X"],
        ),
        (
            r#".Sy "C\p\~ DJ" No "\h'-5n'\p\:F\p" No "\c\p\h'-1n'\~" No "\h'-2n'X""#,
            &["C", "D", "F", "X"],
        ),
        (
            r#".Sy "C\c\p DJ" No "\h'-5n'\p\:F\p" No " \c\p\h'-1n'\~" No "\h'-2n'X""#,
            &["C", "D", "F", "X"],
        ),
        (
            r#".Sy "C\~\p DJ" No "\h'-5n'\p\:F\p" No "\c\p\~" No "\h'-2n'X""#,
            &["C", "D", "F", "X"],
        ),
        (
            r#".Sy "C\p\c DJ" No "\h'-5n'\p\:F\p\~" No "\c\p\~" No "\h'-2n'X""#,
            &["C", "D", "F", "X"],
        ),
        (
            r#".Sy "\cC\p DJ" No "\h'-5n'\p\:F\p" No "\c\p\h'-1n'\~" No "\h'-2n'X""#,
            &["C", "D", "F", "X"],
        ),
        (
            r#".Sy "C\p DJ\c" No "\h'-5n'\p\:F\p" No " \c\p\h'-1n'\~" No "\h'-2n'X""#,
            &["C", "D", "FJ", "X"],
        ),
    ];
    verify_rows(cases);
}

#[test]
fn marked_replay_keeps_only_surviving_positioning_cells() {
    // term.c::term_field queues both term_word's automatic separator and
    // ESCAPE_HORIZ's fixed advances before print. Replaying those writes
    // preserves individual surviving slots, independently of their owners'
    // accrued prefixes and the authored spaces that follow them.
    verify_rows(&[
        (
            r#".Em "A" Em "\z\h'-1n'\h'1n'\h'3n'" Em "F\p\h'-1n'G\p\h'-1n'\c" Em "\h'-10n'X""#,
            &["A  X FG"][..],
        ),
        (
            r#".Sy "A" Em "\z\h'-1n'\h'1n' K" No "F\pG\p\c\h'-2n'" Sy "\h'-9n'X""#,
            &["A K FG"][..],
        ),
        (
            r#".Sy "A" Em "\z\h'-1n'\h'1n' K" Ns No "F\pG\p\c" Ns Em "\h'-9n'X""#,
            &["A KFG"][..],
        ),
        (
            r#".Sy "A" Em "\z\h'-1n'\h'1n' K" Ns No "F\pG\&\p\~\c" No "\h'-4n'X""#,
            &["A   KFX"][..],
        ),
        (
            r#".Sy "A" Em "\z\h'-1n'\h'1n' K" No "F\pG\p\c" No "\h'-4n'X""#,
            &["A   K XG"][..],
        ),
        (
            r#".Sy "A\fP" Em "\z\h'-1n'\h'1n' K" Ns No " F\pG\p\c" No "\h'-4n'X""#,
            &["A   K XG"][..],
        ),
        (
            r#".Em "A" Ns Em "\z\h'-1n'\h'1n'\z" Em "F\pG\p\c\h'-4n'" No "\h'3n'\h'-4n'X""#,
            &["A  G"][..],
        ),
    ]);
}

fn verify_rows(cases: &[(&str, &[&str])]) {
    for &(words, expected) in cases {
        let source = format!("{HEADER}{words}\n{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback =
            json_string_roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
        for document in [&content, &readback] {
            let rendered = mant_render::render_query_man(document);
            let (_, body) = rendered.split_once("CASE0CASE\n").unwrap();
            let (body, _) = body.split_once("CASE999CASE\n").unwrap();
            let rows: Vec<_> = body.trim().lines().map(str::trim).collect();
            assert_eq!(rows, expected, "{words}\n{rendered}");
        }
    }
}

#[test]
fn positioning_prefixes_do_not_spend_body_row_or_cover_offsets() {
    // term_word() buffers the automatic separator next to the HORIZ
    // advance. When the reading prefix emits those cells together, the
    // owner offset advances only at the next glyph. A replayed gap may
    // keep that positioning owner or remain a genuine source scalar.
    // Both offsets still follow term_field()'s accepted rows and paint.
    verify_rows(&[
        (
            r#".Em "A" Em "\z\h'-1n'\h'1n'" E\pm "F\pG\p\c\h'-4n'" No "\h'3n'\h'-4n'X""#,
            &["A   Em", "FG"],
        ),
        (
            r#".Em "A" Em "\z\h'-1n'\h'1n'" \pEm "F\pG\p\c\h'-4n'" No "\h'3n'\h'-4n'X""#,
            &["A   Em", "FG"],
        ),
        (
            r#".Em "A" Em "\z\h'-1n'\h'1n'" \zEm "F\pG\p\c\h'-4n'" No "\h'3n'\h'-4n'X""#,
            &["A   m FG"],
        ),
        (
            r#".Em "A" Em "\z\h'-1n'\h'1n'" E m "F\p G\p\c\h'-4n'" No "\h'3n'\h'-4n'X""#,
            &["A   E m F", "G"],
        ),
        (
            r#".Em "A\zC\fPD\h'-2n'" Em "\z\h'-8n'\h'2n'" Sy "F\pG" Ns Em "\h'-5n'\cX""#,
            &["AD  G"],
        ),
        (
            r#".Em "A" Em "\z\h'-1n'\h'1n'" Em "FW\pG\p\c\h'-4n'" No "\h'3n'\h'-4n'X""#,
            &["A   FWG"],
        ),
        (
            r#".Em "A" Em "\fP\z\h'-1n'\h'1n'" Em "F\pG\p\c\h'-4n'" No "\h'3n'\h'-4n'X""#,
            &["A   FG"],
        ),
        (
            r#".Sy "A" Em "\z\h'-1n'\h'1n' " Sy "\h'3n'F\pG\p\c\h'-1n'" No "\h'-2n'X""#,
            &["A       FX"],
        ),
    ]);
}

#[test]
fn adopted_motion_columns_retire_the_earlier_positioning() {
    // term_word() can walk over two earlier HORIZ cells and their SP;
    // term_field() emits all three once at the same accepted ink. The
    // second motion receipt can replace a column after the first surviving
    // position of the old prefix.
    verify_rows(&[
        (
            r#".Em "A" Em "\z\h'-1n'\h'1n'" Sy "\h'1n'F\pG\p\c\h'-4n'" No "\h'3n'\h'-4n'X""#,
            &["A    FG"],
        ),
        (
            r#".Em "A" Em "\z\h'-1n'\h'1n'" Sy "\h'1n'F\pG\p\c\h'-4n'" No "\h'2n'\h'-3n'X""#,
            &["A    FG"],
        ),
        (
            r#".Em "A" Em "\z\h'-1n'\h'1n'" Sy "\h'1n'F\pG\p\c\h'-4n'" No "\h'4n'\h'-5n'X""#,
            &["A    FG"],
        ),
        (
            r#".Em "A" Em "\z\h'-1n'\h'1n'" Sy "\h'2n'F\pG\p\c\h'-4n'" No "\h'3n'\h'-4n'X""#,
            &["A     FG"],
        ),
        (
            r#".Em "A" Em "\z\h'-1n'\h'1n'" Sy "\h'1n'K\pL\p\c\h'-4n'" No "\h'3n'\h'-4n'X""#,
            &["A    KL"],
        ),
        (
            r#".Em "A" Em "\z\h'-2n'\h'1n'" Sy "\h'1n'F\pG\p\c\h'-4n'" No "\h'3n'\h'-4n'X""#,
            &["A    FG"],
        ),
        (
            r#".Em "A" Em "\z\h'-1n'\h'1n'" Sy "\h'1n'F\pG\p\c\h'-4n'" Ns No "\h'3n'\h'-4n'X""#,
            &["A    FXG"],
        ),
        (
            r#".Em "A" Em "\z\h'-1n'\h'1n'" Sy "\h'1n'F\pG\p\c\h'-4n'" No "\h'-1n'X""#,
            &["A    FG"],
        ),
    ]);
}

#[test]
fn marked_native_decoration_remains_editable_before_flush() {
    // term.c::term_word leaves the earlier marker, SP and local BACKBEFORE
    // in the same short filled buffer. A later HORIZ can overwrite part of
    // a font's raw decoration; term_field decides which cells are printed.
    for (font, ink, columns, expected) in [
        ("Sy", "D", 4, &["DJ"][..]),
        ("Sy", "D", 5, &["D", "J"]),
        ("Sy", "D", 6, &["DD", "J"]),
        ("Sy", "CD", 9, &["CCD", "J"]),
        ("Em", "D", 5, &["_", "J"]),
        ("Em", "CD", 5, &["C_", "J"]),
        ("Em", "CD", 6, &["C_D", "J"]),
        ("No", "CD", 4, &["D", "J"]),
    ] {
        let source = format!(r#".{font} "\p{ink} \zJ" No "\h'-{columns}n'\p""#);
        verify_rows(&[(&source, expected)]);
    }
}

#[test]
fn trailing_native_advance_survives_overlaid_marked_words() {
    // term.c::encode1() gives one bold glyph two native paint copies.
    // term_word() keeps the trailing HORIZ and following graphless
    // operand in the live field. Only term_field() can publish their
    // surviving spaces before ink after the later relative rewind.
    verify_rows(&[
        (
            r#".Sy "A\h'2n'" Em "\z\h'-1n'\h'1n' " Ns No "F\pG\p\c" No "\h'3n'\h'-4n'X""#,
            &["A     FGX"],
        ),
        (
            r#".Sy "A\h'3n'" Em "\z\h'-1n'\h'1n' " Ns No "F\pG\p\c" No "\h'3n'\h'-4n'X""#,
            &["A      FGX"],
        ),
        (
            r#".Sy "A\h'2n'" Em "\h'1n' " Ns No "F\pG\p\c" No "\h'3n'\h'-4n'X""#,
            &["A     FGX"],
        ),
        (
            r#".Sy "A\h'2n'" Em "\z\h'-1n'\h'2n' " Ns No "F\pG\p\c" No "\h'3n'\h'-4n'X""#,
            &["A      FGX"],
        ),
        (
            r#".Sy "A\h'2n'" Em "\z\h'-1n'\h'1n' \h'2n' " Ns No "F\pG\p\c" No "\h'3n'\h'-4n'X""#,
            &["A        FGX"],
        ),
        (
            r#".No "A\h'2n'" Em "\z\h'-1n'\h'1n' " Ns No "F\pG\p\c" No "\h'3n'\h'-4n'X""#,
            &["A     FGX"],
        ),
        (
            r#".Em "A\h'2n'" Em "\z\h'-1n'\h'1n' " Ns No "F\pG\p\c" No "\h'3n'\h'-4n'X""#,
            &["A     FGX"],
        ),
        (
            r#".Sy "A\h'2n'" Em "\z\h'-1n'\h'1n' " Ns No "J\pK\p\c" No "\h'3n'\h'-4n'X""#,
            &["A     JKX"],
        ),
    ]);
}
