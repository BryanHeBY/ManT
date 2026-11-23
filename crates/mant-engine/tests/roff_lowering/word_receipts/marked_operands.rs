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
            assert_eq!(
                literal_rows_relative_to_label(&rendered),
                expected,
                "{source}\n{rendered}"
            );
        }
    }
}

fn literal_rows_relative_to_label(rendered: &str) -> Vec<&str> {
    let (_, body) = rendered.split_once("CASE0CASE\n").unwrap();
    let (body, _) = body.split_once("CASE999CASE\n").unwrap();
    let label = body
        .lines()
        .find(|line| line.trim_start() == "TERM")
        .unwrap();
    let prefix = label.strip_suffix("TERM").unwrap();
    body.strip_suffix(prefix)
        .unwrap_or(body)
        .trim_end_matches('\n')
        .lines()
        .map(|line| line.strip_prefix(prefix).unwrap_or(line))
        .collect()
}

#[test]
fn graphless_literal_retreat_restores_temporary_origin_on_real_endline() {
    // term.c::ESCAPE_HORIZ changes both offset and ti for the excess
    // beyond the complete native col. print_mdoc_node() restores only
    // offset, then ascii_endline() consumes the retained ti. Relative
    // geometry must survive a later word and another no-fill source row.
    for (kind, width, indent, clipped_indent) in [
        ("hang", 0, 2, 9),
        ("hang", 4, 6, 17),
        ("tag", 0, 2, 9),
        ("tag", 4, 6, 17),
    ] {
        for (retreat, prefix, mode, next, prior, empty, target_column) in [
            ("-3n", "", "", "", "START", true, indent),
            ("-3.5n", "", "", "", "START", true, indent),
            ("-3.75n", "", "", "", "START", true, indent + 1),
            ("-4n", "", "", "", "START", true, indent + 1),
            ("-5.5n", "", ".ti 2n\n", "", "START", true, indent + 2),
            ("-5n", "", "", ".No Y\n", "START", true, indent + 2),
            ("-100n", r"\c", "", "", "TART", false, clipped_indent),
        ] {
            let controls = if retreat.contains('.') || !empty {
                r"\&\p"
            } else {
                r"\p\&"
            };
            let source = format!(
                r#"{HEADER}.nf
.Bl -{kind} -width {width}n
.It TERM
.No START{prefix}
{mode}.No "\h'0n'{controls}" No "\h'{retreat}'\p"
.No X
{next}.El
.fi
{END}"#,
            );
            let content =
                json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
            let rendered = mant_render::render_query_man(&content);
            let mut expected = vec![
                String::new(),
                "TERM".into(),
                format!("{}{prior}", " ".repeat(indent)),
            ];
            if empty {
                expected.push(String::new());
            }
            let next_indent = " ".repeat(target_column);
            expected.push(format!("{next_indent}X"));
            if !next.is_empty() {
                expected.push(format!("{next_indent}Y"));
            }
            assert_eq!(
                literal_rows_relative_to_label(&rendered),
                expected,
                "{source}\n{rendered}"
            );
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

#[test]
fn covered_trailing_advance_keeps_the_live_native_buffer() {
    // term_word() can overwrite the last deferred advance without
    // ending the field. Its next word still paints against the original
    // native buffer. term_field() exposes B, which covers both A glyphs.
    verify_rows(&[
        (
            r#".Sy "A\h'1n'" Em "\z\h'-1n'\h'1n' " Ns No "F\pG\p\c" No "\h'3n'\h'-4n'X""#,
            &["A    FGX"],
        ),
        (
            r#".Sy "A\h'1n'" No "\h'1n'\h'-4n'\zABC" Ns No "\zFG\h'2n'\p\h'2n'" Ns Sy "\h'1n'\h'-2n'\fP\h'1n'X""#,
            &["BCG    X"],
        ),
        (
            r#".Sy "A\h'1n'" No "\h'1n'\h'-4n'\zABC" Ns No "\zFG\h'2n'\h'2n'" Ns Sy "\h'1n'\h'-2n'\fP\h'1n'X""#,
            &["BCG    X"],
        ),
        (
            r#".Sy "A\h'1n'" No "\h'1n'\h'-4n'\zABC" Ns No "\zFG\h'3n'\p\h'2n'" Ns Sy "\h'1n'\h'-2n'\fP\h'1n'X""#,
            &["BCG     X"],
        ),
        (
            r#".Sy "A\h'1n'" No "\h'1n'\h'-4n'\zABC" Ns No "\zFG\h'2n'\p\h'3n'" Ns Sy "\h'1n'\h'-2n'\fP\h'1n'X""#,
            &["BCG     X"],
        ),
        (
            r#".Sy "A\h'1n'" No "\h'1n'\h'-4n'\zABC" Ns No "\zJK\h'2n'\p\h'2n'" Ns Sy "\h'1n'\h'-2n'\fP\h'1n'X""#,
            &["BCK    X"],
        ),
        (
            r#".Sy "A\h'1n'" No "\h'1n'\h'-4n'\zABC" Ns No "\zFG\h'2n'\p\h'2n'" Ns No "\h'1n'\h'-2n'\fP\h'1n'X""#,
            &["BCG    X"],
        ),
    ]);
}

#[test]
fn closed_definition_rows_preserve_accepted_underline_strokes() {
    // term.c::term_field paints the unpaired stroke after C. The device
    // closes the row either in the pass loop or at the TAG HEAD post
    // (mdoc_term.c::termp_it_post). A HANG field that keeps the row open
    // can still be covered by a later BODY field at the same column.
    let cases = [
        (
            r#".Bl -hang -width 0n
.It TERM
.Em "\h'1n'C\pJ" No "\h'-2n'\p"
.No X
.El"#,
            &["TERM  C_", "X"][..],
        ),
        (
            r#".Bl -hang -width 4n
.It TERM
.Em "\h'1n'C\pJ" No "\h'-2n'\p"
.No X
.El"#,
            &["TERM   C_", "X"][..],
        ),
        (
            r#".Bl -tag -width 4n
.It TERM
.Em "\h'1n'C\pJ" No "\h'-2n'\p"
.No X
.El"#,
            &["TERM   C_", "X"][..],
        ),
        (
            r#".Bl -tag -width 4n
.It Xo TERM
.br
.Em "\h'1n'C\pJ" No "\h'-2n'\p"
.Xc
.No X
.El"#,
            &["TERM   C_", "X"][..],
        ),
        (
            r#".Bl -hang -width 3n
.It Em "\zC\pJ" Sy "\h'-2n'\p"
.No "\zB\zY"
.br
.No "X"
.El"#,
            &["Y", "X"][..],
        ),
        (
            r#".Bl -hang -width 3n
.It Em "\zC\pJ" Sy "\h'-2n'\p"
.No "\h'-1n' \zB\zY"
.br
.No "X"
.El"#,
            &["Y", "X"][..],
        ),
    ];
    for (words, expected) in cases {
        let source = format!("{HEADER}{words}\n{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback =
            json_string_roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
        let rows_of = |rendered: &str| {
            let (_, body) = rendered.split_once("CASE0CASE\n").unwrap();
            let (body, _) = body.split_once("CASE999CASE\n").unwrap();
            body.trim()
                .lines()
                .map(|line| line.trim().to_owned())
                .collect::<Vec<_>>()
        };
        let rendered = mant_render::render_query_man(&content);
        assert_eq!(rows_of(&rendered), expected, "{words}\n{rendered}");

        // Markdown uses list markers and relative placement. Preserve the
        // confirmed ink and its physical-row boundary through readback.
        let rendered = mant_render::render_query_man(&readback);
        let words_on_row = |row: &str| row.split_whitespace().collect::<Vec<_>>().join(" ");
        let rows: Vec<_> = rows_of(&rendered)
            .iter()
            .map(|row| words_on_row(row.strip_prefix("• ").unwrap_or(row)))
            .collect();
        assert_eq!(
            rows,
            expected
                .iter()
                .map(|row| words_on_row(row))
                .collect::<Vec<_>>(),
            "{words}\n{rendered}"
        );
    }
}

#[test]
fn fresh_words_controls_and_split_bold_copies_keep_raw_slots() {
    // Verified against pinned reference mandoc (-Tascii and -Tutf8):
    // - Fresh decorated/switched words with spaces or zero-width controls
    //   buffer exact raw bytes before term_fill() (term.c:621-667, 899-969).
    // - ESCAPE_NOSPACE (\c) cancels BACKAFTER without buffering bytes and
    //   sets continuation only at word end (term.c:662-667).
    // - Positive HORIZ in a fresh filled word writes NBRSP or consumes
    //   BACKBEFORE at word end (term.c:678-699).
    // - Trailing spaces after \p advance col without overwriting existing
    //   cells when col < lastcol (term.c:868-871).
    // - Overwriting the backspace between a bold glyph's two copies leaves
    //   both copies visible in the accepted row (term.c:915-925).
    verify_rows(&[
        (r#".Sy " D J " Sy "\h'-8n'\&\p\:F\p""#, &["D", "F"]),
        (r#".Sy " DJ\c" Sy "\h'-8n'\&\p\:F\p""#, &["F"]),
        (r#".Sy " D\&J" Sy "\h'-8n'\&\p\:F\p""#, &["F"]),
        (r#".Sy " D\:J" Sy "\h'-8n'\&\p\:F\p""#, &["F"]),
        (r#".Sy " \fRDJ" Sy "\h'-8n'\&\p\:F\p""#, &["F"]),
        (r#".Sy "D\z\c J" Ns No "\h'-5n'\p ""#, &["D", "J"]),
        (r#".Sy "D \zJ\c" Ns No "\h'-5n'\p ""#, &["DD", "J"]),
        (r#".Sy "\h'1n'C\zJ" Ns No "\h'-5n'\p ""#, &["CJ"]),
        (r#".Sy "C\zJ\h'1n'" Ns No "\h'-5n'\p ""#, &["CCJ"]),
        (r#".Sy "CD \zJ\h'1n'" Ns No "\h'-5n'\p ""#, &["CD", "J"]),
        (r#".Sy "D \zJ" Ns No "\h'-5n'\p ""#, &["DD", "J"]),
        (r#".No "\h'-3n'BC\p" Ns No "\h'-3n'Y""#, &["YC"]),
        (
            r#".Sy "A\c" Em "\z\h'-1n'\h'1n'" Sy "F\p\h'-1n'G\p\h'-1n'\c\h'-1n'" No "\h'-4n'X ""#,
            &["A  XG"],
        ),
        (r#".Sy "CD \zJ" Ns No "\h'-8n'\& \pX""#, &["CCXD", "J"]),
        (r#".Sy "CD \zJ" Ns Em "\h'-8n'\& \pX""#, &["CCXJ"]),
        (r#".Sy "CD \zJ" Ns No "\h'-8n'\&\p X""#, &["CDXD", "J"]),
        (r#".Sy "CD \zJ" Ns No "\h'-7n'\&\pX""#, &["XD", "J"]),
    ]);
}

#[test]
fn fresh_motion_nbsp_and_rearmed_words_preserve_raw_slots() {
    // Verified against pinned reference mandoc (-Tascii and -Tutf8):
    // - Fresh decorated words ending in \p or containing UTF-8 U+00A0 alongside
    //   ordinary SP keep raw_edit_fence at 0 (term.c:620-661, 899-969).
    // - A re-armed local word (\zJ\z) followed by a control-only HORIZ cancels
    //   BACKAFTER before the second HORIZ retreats (term.c:678-682, 701-705).
    // - Fresh filled words with leading, inner, or trailing positive HORIZ
    //   retain both reading spaces beside authored SP and multi-cell
    //   FixedDrawingAdvance overprints (term.c:389-434, 678-699, 915-925).
    verify_rows(&[
        (r#".Sy " DJ\p" Sy "\h'-8n'\&\p\:F\p""#, &["F"]),
        (r#".Sy " DJ\p" Sy "\c\h'-8n'\&\p\:F\p""#, &["F"]),
        (r#".Sy " \~DJ" Sy "\h'-8n'\&\p\:F\p""#, &["F"]),
        (r#".Sy "\0 DJ" Sy "\h'-8n'\&\p\:F\p""#, &["F"]),
        (r#".Sy " D\0J\0" Sy "\h'-8n'\&\p\:F\p""#, &["D", "F"]),
        (r#".Sy " D\~J\~" Sy "\h'-8n'\&\p\:F\p""#, &["D", "F"]),
        (r#".Sy "D \zJ\z" No "\h'-5n'\h'-2n'\p""#, &["D JJ"]),
        (r#".Sy " DJ\h'1n'" Sy "\h'-8n'\&\p\:F\p""#, &["F"]),
        (r#".Em " DJ\h'1n'" Sy "\h'-10n'\&\p\:F\p""#, &["F"]),
        (r#".Sy "\h'2n' DJ" Sy "\h'-4n'\&\p\:F\p""#, &["D", "F"]),
        (r#".Sy "\h'3n' DJ" Sy "\h'-8n'\&\p\:F\p""#, &["F"]),
        (r#".Sy "C\h'1n'\zJ\h'1n'" No "\h'-7n'\&""#, &["CC J"]),
        (r#".Sy "C\h'3n'\zJ\h'3n'" No "\h'-10n'\&""#, &["J"]),
        (r#".Sy "D\h'2n' \zJ\h'2n'" No "\h'-5n'\p""#, &["D  J"]),
        (r#".Sy "D\h'3n' \zJ\h'3n'" No "\h'-5n'\p""#, &["D    JJ"]),
        (r#".Sy "\h'3n'\~D \zJ" Ns No "\h'-1n'\p\fP""#, &["D JJ"]),
        (r#".Sy "\h'3n'\~D \zJ" Ns Em "\h'-1n'\p\h'3n'""#, &["D JJ"]),
    ]);
}

#[test]
fn definition_list_heads_preserve_native_slot_edits_and_deferred_breaks() {
    // Verified against pinned reference mandoc (-Tutf8):
    // - Scoped (-tag/-hang) and DefinitionLine (-inset) HEAD words keep raw
    //   bold-copy aliases and marked decoration slots before term_flushln()
    //   (mdoc_term.c:760-838, term.c:915-929).
    // - Inset HEADs whose word scan awaits flush still hand off deferred
    //   hard-row breaks to head_presentation when BODY flushes (term.c:205-220).
    let cases = [
        (
            r#".Bl -tag -width 2n
.It Sy "C D" Sy "\h'-7n'\&"
.No "\h'-5n'B"
.El"#,
            &["CC D", "B"][..],
        ),
        (
            r#".Bl -hang -width 3n
.It Em "C\p\zJ" Em "\h'-4n'\pJ"
.No "B"
.El"#,
            &["CJ   B"][..],
        ),
        (
            r#".Bl -inset
.It Em "A" Ns No "\h'-3n'\z"
.No " B"
.El"#,
            &["B"][..],
        ),
        (
            r#".Bl -inset
.It Em "\~C\pJ" No "\h'-2n'\p"
.br
.No X
.El"#,
            &["C", "X"][..],
        ),
        (
            r#".Bl -inset
.It Em "\~C\pJ" No "\fP\p\(hy"
.br
.No X
.El"#,
            &["CJ", "‐", "X"][..],
        ),
        (
            r#".Bl -inset
.It Em "\z\z\pJ" No "\fP\p"
.br
.No X
.El"#,
            &["J", "", "X"][..],
        ),
    ];
    for (words, expected) in cases {
        let source = format!("{HEADER}{words}\n{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback =
            json_string_roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
        let rows_of = |rendered: &str| {
            let (_, body) = rendered.split_once("CASE0CASE\n").unwrap();
            let (body, _) = body.split_once("CASE999CASE\n").unwrap();
            body.trim()
                .lines()
                .map(|line| line.trim().to_owned())
                .collect::<Vec<_>>()
        };
        let rendered = mant_render::render_query_man(&content);
        assert_eq!(rows_of(&rendered), expected, "{words}\n{rendered}");

        let rendered = mant_render::render_query_man(&readback);
        let words_on_row = |row: &str| row.split_whitespace().collect::<Vec<_>>().join(" ");
        let rows: Vec<_> = rows_of(&rendered)
            .iter()
            .map(|row| words_on_row(row.strip_prefix("• ").unwrap_or(row)))
            .collect();
        assert_eq!(
            rows,
            expected
                .iter()
                .map(|row| words_on_row(row))
                .collect::<Vec<_>>(),
            "{words}\n{rendered}"
        );
    }
}

#[test]
fn non_list_slot_boundaries_preserve_fences_advances_and_retreats() {
    // Verified against pinned reference mandoc (-Tutf8; -Tascii agrees on
    // every row once U+00A0 is normalized to ASCII space):
    // - Empty, space-only, \z\h-canceled, and mid-word \c decorated words
    //   write no unproved raw slots and keep raw_edit_fence intact
    //   (term.c:559-590, 662-682, 899-969).
    // - Non-fresh bounded words ending in positive HORIZ and blank-tail
    //   \z\h'-1n'\h'1n' keep exact expanded font-slot budgets within
    //   MAX_EDITABLE_APPEND_CELLS (term.c:678-699, 899-930).
    // - Post-\p relative retreats retain the live retreated cursor and hold
    //   pass acceptance until term_flushln(), preserving nbr == 0 rejection
    //   and overprint coverage (term.c:143-146, 659-660, 701-718).
    // - U+00A0 (\~) is printable ink in -Tutf8 that releases preceding inner
    //   HORIZ advances (term.c:345-349, 416-441).
    // - Overwritten underline backspaces release unpaired '_' in Filled
    //   flow, and decoration pairs (_\b / c\b) before ink do not hide an
    //   earlier device gap (term.c:389-441, 911-921).
    verify_rows(&[
        // NL-1: non-fresh bounded trailing \h'+Nn' and blank-tail \z\h'-1n'\h'1n'
        (r#".Sy "A" No "FG\h'2n'" No "\h'-5n'X""#, &["A XG"]),
        (r#".Em "A" No "FG\h'2n'" No "\h'-3n'X""#, &["A FGX"]),
        (r#".Sy "A" Em "\z\h'-1n'\h'1n'" No "X""#, &["A   X"]),
        (r#".Sy "A\h'2n'" No "FG\h'2n'" No "X""#, &["A   FG   X"]),
        (r#".Sy "A" No "FG\h'2n'" Ns No "\h'-4n'X""#, &["A XG"]),
        (r#".Em "A" No "FG\h'2n'\c" No "\h'-3n'X""#, &["A FX"]),
        // NL-2 & NL-3: empty / \z\h-canceled / mid-word \c decorated words
        (r#".Sy "F\pAB\c\h'-1n'" No "X""#, &["FABX"]),
        (r#".Sy "F\pAB\c\h'-1n'" Ns No "X""#, &["FAX"]),
        (r#".Sy "A" Sy "" No "\h'-4n'X""#, &["AXA"]),
        (r#".Sy "A" Em "" No "\h'-4n'X""#, &["AXA"]),
        (r#".Sy "A" Em "\z\h'1n'" No "\h'-4n'X""#, &["AXA"]),
        (r#".Sy "A" Em "\z\h'2n'" No "\h'-4n'X""#, &["AXA"]),
        (r#".Sy "A" Em "\z\h'1n' " No "\h'-5n'X""#, &["AXA"]),
        (r#".Sy "A" Em "\z\h'-1n'" No "\h'-4n'X""#, &["AXA"]),
        (r#".Sy "A" Sy "B\cC" No "\h'-6n'X""#, &["A BXBC"]),
        (r#".Sy "A" Sy "B\z\h'1n'C" No "\h'-6n'X""#, &["A BXBC"]),
        // NL-4c: post-\p retreated cursor
        (r#".Sy "C\p DJ\h'-1n'" No "X""#, &["C", "DJX"]),
        (r#".Sy "C\p DJ\h'-1n'" Ns No "X""#, &["C", "DX"]),
        (r#".Sy "C\p DJ\h'-2n'" Ns No "X""#, &["C", "DJXJ"]),
        // NL-7a/b: U+00A0 inner advances
        (r#".No "FG\h'2n'\~" No "Z""#, &["FG  \u{a0} Z"]),
        (r#".Sy "FG\h'2n'\~" No "Z""#, &["FG  \u{a0} Z"]),
        (r#".Em "FG\h'2n'\~" No "Z""#, &["FG  \u{a0} Z"]),
        (r#".Sy "A" No "FG\h'2n'\~" No "Z""#, &["A FG  \u{a0} Z"]),
        (r#".No "FG\h'2n'\~" Ns No "\h'-2n'X""#, &["FG X"]),
        // NL-8a/b: non-leading negative \h live prefix and nbr == 0 rejection
        (r#".No "A\p " Ns Sy "X\h'-4n'""#, &["A", "X"]),
        (r#".No "\p " Ns Sy "X\h'-4n'" No "Z""#, &[]),
        (r#".No "A\p " Em "\fPX\h'-4n'""#, &["A", "X"]),
        (r#".No "\p " Em "\fPX\h'-4n'" No "Z""#, &["Z", "X"]),
        (r#".No "A\p " Ns Sy "X\h'-4n'" No "Z""#, &["A", "X"]),
        // NL-9a/b/c: unpaired underlines, decoration gap skip, trailing entry blanks
        (r#".Em "F" No "\h'-3n'X""#, &["_XF"]),
        (r#".Em "F" Ns No "\h'-2n'X""#, &["_XF"]),
        (r#".No "A\h'2n'" Em "X""#, &["A   X"]),
        (r#".No "A\h'2n'" Ns Em "X""#, &["A  X"]),
        (r#".Em "A" Em "\z\h'-1n'\h'1n'" Sy "X""#, &["A   X"]),
        (r#".Em "A" No "B" No "\h'-3n'X""#, &["AXB"]),
        (r#".Sy "A" No "B" No "\h'-3n'X""#, &["AXB"]),
    ]);
}

#[test]
fn positive_matrix_slot_interactions_preserve_advances_and_retreats() {
    // Verified against pinned reference mandoc (-Tutf8): sorted deferred_blanks,
    // decoration-skip guard on projected blanks, post-\p retreated cursor with
    // later ink, bufferc traversing unprojected Earlier(0) blanks, backline
    // target on ProveRawSlots, and inner+trailing HORIZ before \p\c
    // (term.c:389-441, 659-660, 678-718, 868-871, 901-921).
    verify_rows(&[
        (
            r#".Ns Em "A\h'1n'" Em "\z\h'-1n'\h'1n'" Ns No " F\zG\p\c" Sy "\h'-4n'X""#,
            &["A  XG"],
        ),
        (
            r#".Ns Em "A\h'1n'\h'1n'" Em "\z\h'-1n'\fR" Ns No " F\pG\p\c" Sy "\h'-4n'X""#,
            &["A    X"],
        ),
        (
            r#".Sy "AB" No "\0\h'-4n'\z\p\zC\h'1n'" Ns No "\zFG\h'2n'\p\h'2n'" Ns Sy "\h'1n'\h'-2n'\fPX""#,
            &["ABCG   X"],
        ),
        (
            r#".Sy "AB" No "Z\h'-6n'\z\p\:C" Sy "\zFG\h'2n'\p\~" Ns Sy "\h'1n'\h'-2n'\fPX\h'-4n'""#,
            &["A", "CG  X"],
        ),
        (
            r#".Sy "A" Sy "\:\h'1n'\h'-6n'\z\p\cC" Ns No "F\h'2n'\~" Ns Sy "\h'1n'\h'-1n'X""#,
            &["CF  \u{a0}X"],
        ),
        (
            r#".Em "A" Em "\z\z\h'-1n'\h'1n'" Em " F\p\h'-1n'\h'-1n'G\p\h'-1n'\c" Em "\h'-10n'X""#,
            &["X  G"],
        ),
        (
            r#".Em "A\zC\fPD\p" Em "\z\h'-8n'\h'2n'" Ns No "F\pG" Ns Em "\h'-5n'\cX""#,
            &["AD", "XG"],
        ),
        (
            r#".Em "A\zC\fPD" Ns Em "\fP\h'-8n'\fBF\pG" Em "\h'0n'\&" No "\h'-1n'\z\h'-5n'\cX""#,
            &["FGDX"],
        ),
        (
            r#".Sy "A\zC\zD" Ns Em "\c\h'-8n'\z\pC\h'3n'" Em "\&\:" Ns No " \h'-3n' X""#,
            &["ACDX"],
        ),
        (
            r#".No "A\z\zC\z\fPD\z" Em "\:\h'-10n'\:" Ns Sy "D  " Em "\h'-1n'\fPX""#,
            &["ADD  X"],
        ),
        (
            r#".Sy "A" Em "\z\h'-1n'\h'1n' " Ns Sy "F\p\h'1n'G\p\h'1n'\c" Sy "\z\h'-4n'X""#,
            &["A   F G X"],
        ),
        (
            r#".Em "A\p" Em "\z\h'-1n'\h'1n'" Ns No "F\h'2n'\pG\h'2n'\p\c" No "\h'-2n'X""#,
            &["A", "F  G X"],
        ),
    ]);
    // Inset run-in head with rewound body \& followed by decorated word must
    // not double-emit the projected separator across X\b (term.c:389-434).
    for body in [
        ".Bl -inset\n.It Sy \"C\\&V\" No \"\\h'-1n' X\"\n.No \"\\h'-5n'\\&\"\n.Sy \"X\"\n.El",
        ".Bl -inset\n.It Sy \"C\\&V\" No \"\\h'-1n' X\"\n.No \"\\h'-1n' \\h'-5n'\\&\"\n.Sy \"X\"\n.El",
    ] {
        let source = format!("{HEADER}{body}\n{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let rendered = mant_render::render_query_man(&content);
        let (_, section) = rendered.split_once("CASE0CASE\n").unwrap();
        let (section, _) = section.split_once("CASE999CASE\n").unwrap();
        let rows: Vec<_> = section
            .trim()
            .lines()
            .map(|line| line.trim().trim_start_matches("• "))
            .collect();
        assert_eq!(rows, ["C X"], "{body}\n{rendered}");
    }
}

#[test]
fn positive_matrix_retreated_advances_and_deferred_runs() {
    // Verified against pinned reference mandoc (-Tutf8 and -Tascii; in A5,
    // \0 produces U+00A0 in -Tutf8 and ASCII space in -Tascii, trimmed by
    // verify_rows at the left margin):
    // - Trailing entry blanks alone do not force a retreated-cursor word into
    //   unprojected geometry, while interior surviving entry blanks promote to
    //   spaces and unread forward advances stay editable (term.c:659-660,
    //   678-718, 899-930).
    // - Post-advance \z<glyph>\h'+Nn' clears BACKBEFORE on the first unit of
    //   positive relative HORIZ, and \: breakpoints preserve inner-advance
    //   publication (term.c:692-698).
    // - Live replay from a fresh un-fenced field crosses multi-cell deferred
    //   HORIZ runs cleanly before later negative retreats (term.c:678-718).
    verify_rows(&[
        // Micro-pattern A: trailing \h'+Nn' + \: / unread inner advances
        (
            r#".Em "A\zC\fPD" No "\h'-1n'\h'-4n'\h'1n'F\pG" No "F\zG\h'2n'\:" No "\p\h'0n'\fPX""#,
            &["AFG", "FG  X"],
        ),
        (
            r#".Em "A\zC\fPD" No "\h'-1n'\h'-4n'\h'1n'F\pG" No "F\zG\h'2n'\:" No "\p\h'1n'\fPX""#,
            &["AFG", "FG   X"],
        ),
        (
            r#".Em "A\zC\fPD" No "\h'-1n'\h'-4n'\h'1n'F\h'3n'\pG\h'3n'\h'-1n'" Ns Em "\h'3n'F\zG\h'2n'\:\h'3n'" No "\p\h'-1n'\fPX""#,
            &["AD G     FG", "X"],
        ),
        (
            r#".Em "\0A\zC\fPD" No "\h'-1n'\h'-4n'\h'1n'F\pG" No "F\h'1n'\zG\h'1n'\h'2n'\:" Ns No "\p\h'-2n'\fPX""#,
            &["AFG", "F G  X"],
        ),
        // Micro-pattern B: fresh multi-cell deferred \h'+Nn' runs with \p/\z retreats
        (
            r#".Em "A\zC\fPD" Em "\z\h'-8n'\h'2n'" Ns No "F\p\zG" Ns Em "\h'-5n'\cX""#,
            &["ADXFG"],
        ),
        (
            r#".Em "A\zC\fPD" Em "\z\h'-8n'\h'2n'\h'3n'" Ns No "F\z\pG" Ns Em "\h'-5n'\cX""#,
            &["AD   XFG"],
        ),
        (
            r#".Em "A\zC\fPD" Em "\z\h'-8n'\h'2n'" Ns No "F\p\zG" Ns Em "\p\h'-5n'\cX""#,
            &["AD XG"],
        ),
        // Retreated cursor before a trailing EntryBlank stays live so a
        // following Ns word overwrites the blank and cancels the \p break
        // (term.c:288-307, 706-707).
        (
            r#".Sy "AB" No "\h'1n'\h'-4n'\z\h'3n'\pC" Ns No "\zFG\h'2n'\~\p\h'2n'" Ns Sy "\h'1n'\h'-2n'\fPX""#,
            &["ABCG  \u{a0} X"],
        ),
        (
            r#".Sy "AB" No "\h'-3n'\pC" Ns No "\zFG\h'2n'\~\p\h'2n'" Ns Sy "\h'1n'\h'-2n'\fPX""#,
            &["ABCG  \u{a0} X"],
        ),
        (
            r#".Em "A\zC\fPD" No "\h'-1n'\h'-4n'\h'1n'F\pG" Ns No "F\zG\h'2n'\:" No "\p\h'0n'\fPX""#,
            &["AFGFG", "X"],
        ),
        // Overwritten intermediate owner rebased onto a decorated restart word
        // keeps the preceding \p break; \u{a0} trailing advance deferral does
        // not double-defer an earlier word's NBRSP; marker-bearing retreated
        // words keep overprint and trailing advance counts (term.c:205-220,
        // 345-349, 692-698, 868-869).
        (
            r#".Sy "C\p DJ" No "\h'-5n'\p\:F\p" No "\c\p\~" Ns Em "\h'-2n'X""#,
            &["C", "D", "F", "X"],
        ),
        (
            r#".Sy "C\p DJ" No "\h'-5n'\p\:F\p" No "\c\p\h'-1n'\~" Sy "\h'-2n'X""#,
            &["C", "D", "F", "X"],
        ),
        (
            r#".Ns Em "C\pDJ" No "\h'-5n'\p\:F\p" No "\p\~" Ns Sy "\h'-2n'X\h'-4n'""#,
            &["C_", "F", "X"],
        ),
        (
            r#".Sy "C\p DJ" Ns Em "\h'-5n'\p\:F\p\h'2n'" Ns Sy "\c\p\~" No "\h'-2n'X""#,
            &["C", "D", "F  X"],
        ),
        (
            r#".No "AB" No "\h'1n'\h'-6n'\zZC" Ns No "\zF\(hyG\h'2n'\p \h'2n'" Ns Sy "\h'1n'\h'-2n'\fPX""#,
            &["C\u{2010}G", "X"],
        ),
        (
            r#".No "AAB" No "\0\h'1n'\h'-6n'\z\pC" Ns No "\zFG\h'2n'\p\h'2n'" Ns Sy "\h'1n'\z\h'-2n'\fPX""#,
            &["G     X"],
        ),
        (
            r#".No "AB" No "\h'1n'\~\h'-6n'\z\pC" Ns No "\zFG\h'2n'\p\p\h'2n'" Ns Sy "\h'1n'\z\h'-2n'\fPX""#,
            &["G     X"],
        ),
    ]);
}

fn verify_definition_list_cases(cases: &[(&str, &[&str])]) {
    let rows_of = |rendered: &str| {
        let (_, body) = rendered.split_once("CASE0CASE\n").unwrap();
        let (body, _) = body.split_once("CASE999CASE\n").unwrap();
        let mut rows: Vec<_> = body.lines().map(|line| line.trim().to_owned()).collect();
        while rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        rows
    };
    let words_on_row = |row: &str| row.split_whitespace().collect::<Vec<_>>().join(" ");
    for &(words, expected) in cases {
        let source = format!("{HEADER}{words}\n{END}");
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback =
            json_string_roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
        let rendered = mant_render::render_query_man(&content);
        assert_eq!(rows_of(&rendered), expected, "{words}\n{rendered}");

        let rendered = mant_render::render_query_man(&readback);
        let rows: Vec<_> = rows_of(&rendered)
            .iter()
            .map(|row| words_on_row(row.strip_prefix('•').unwrap_or(row)))
            .filter(|row| !row.is_empty())
            .collect();
        let expected_nonempty: Vec<_> = expected
            .iter()
            .filter(|row| !row.is_empty())
            .map(|row| words_on_row(row))
            .collect();
        assert_eq!(rows, expected_nonempty, "{words}\n{rendered}\n{markdown}");
    }
}

#[test]
fn definition_list_replays_and_marked_operands_preserve_edits() {
    // Verified against pinned reference mandoc (-Tutf8; -Tascii also checked
    // on every case; inset's generated BODY `\ ` separator renders as ASCII
    // space per row_handoff.rs):
    // - DL-3: Replayed body slots after \z<glyph> include their surviving
    //   automatic separator in anchor.content so trailing \pZ keeps its
    //   scalar quota (term.c:573-589, 903-908).
    // - DL-8 & DL-4 #2/#3: Fresh DefinitionLine operands with leading
    //   negative relative HORIZ or inner positive HORIZ before \p keep
    //   bounded editable raw slots until term_flushln() (term.c:659-660,
    //   678-718).
    // - DL-6/7: Bounded marked HEAD operands across DefinitionLine and
    //   Scoped fields defer pass commitment until flush, rebasing retired
    //   row origins/presentations past omitted scalars; inset's generated
    //   U+00A0 overwrites a live BreakMarker at col (term.c:205-220, 922-923).
    verify_definition_list_cases(&[
        // DL-3
        (
            ".Bl -hang -width 3n\n.It Sy \"CJ\" Sy \" \"\n.No \"\\zB\" No \"\\pZ\"\n.El",
            &["CJ   BZ"],
        ),
        (
            ".Bl -tag -width 2n\n.It Sy \"CJ\" No \" \"\n.No \" B\\zY\"\n.Sy \"\\pZ\"\n.El",
            &["CJ", "BYZ"],
        ),
        (
            ".Bl -inset\n.It No \"CJ\" Em \"\"\n.No \"B\\zY\"\n.No \"\\pZ\"\n.El",
            &["CJ  BYZ"],
        ),
        // DL-8
        (
            ".Bl -tag -width 2n\n.It Sy \"C-D\" Sy \"\"\n.No \"\\h'-1n' B\\pV\" No \"\\h'-2n'Y\"\n.El",
            &["C-D", "BY"],
        ),
        (
            ".Bl -tag -width 2n\n.It Em \"CV\" Em \"\"\n.No \"\\h'-1n' B\\pV\" No \"\\h'-3n'Y\"\n.El",
            &["CV", "BYV"],
        ),
        (
            ".Bl -hang -width 3n\n.It Em \"C-D\" No \" \"\n.No \"\\h'-1n' B\\pV\" No \"\\h'-3n'Y\"\n.El",
            &["C-D   BYV"],
        ),
        // DL-4 #3 & #2
        (
            ".Bl -inset\n.It Em \"\\h'-1n'C\\pJ\" No \"\\h'-2n'\\p\"\n.Bd -literal\n.No X\n.Ed\n.El",
            &["C", "", "X"],
        ),
        (
            ".Bl -inset\n.It Em \"C\\h'1n'\\pJJ\" No \"\\h'-2n'\\p\"\n.Pp\n.No X\n.El",
            &["C J", "", "X"],
        ),
        // DL-6/7
        (
            ".Bl -inset\n.It Em \"C\\p J\" No \"\\h'-7n'F\"\n.No \"\" No \"Y\"\n.El",
            &["F  J"],
        ),
        (
            ".Bl -hang -width 3n\n.It Sy \"C\\p J\" Em \"\\h'-6n'F\"\n.No \"BC\" No \"Y\"\n.El",
            &["CJ   BC Y"],
        ),
        (
            ".Bl -tag -width 2n\n.It Sy \"C\\p J\" No \"\\h'-7n'F\"\n.No \"BY\"\n.El",
            &["F", "J", "BY"],
        ),
    ]);
}

#[test]
fn definition_list_paint_origins_viscol_gaps_and_run_in_overlaps() {
    // Verified against pinned reference mandoc (-Tutf8 and -Tascii; inset's
    // generated BODY `\ ` separator renders as ASCII space per row_handoff.rs):
    // - DL-6/7 #4 & DL-10: Definition HEADs record fresh-row device paint
    //   origins, prove trailing unpaired Backline overhangs, and carry the
    //   exact native viscol gap when term.c:434 subtracts getwidth(NBRZW)==0
    //   (term.c:389-442, term_ascii.c:199-242).
    // - DL-9 #2: Inset run-in overlap over a single-cell bold copy replaces
    //   that slot with U+00A0 and retires its alias (term.c:915-925).
    // - Unprojected-blank and empty intermediate passes across HEAD/BODY keep
    //   their authored \p row breaks (term.c:205-220).
    verify_definition_list_cases(&[
        // DL-6/7 #4 & DL-10
        (
            ".Bl -tag -width 2n\n.It Sy \"\\zC\\pJ\" No \"\\h'-7n'\\&\"\n.No \"\"\n.br\n.No \"Y\"\n.El",
            &["JC", "Y"],
        ),
        (
            ".Bl -tag -width 2n\n.It Sy \"CV\" No \"\\h'-2n'\\&\"\n.No \" B\"\n.br\n.No \"Z\"\n.El",
            &["C    B", "Z"],
        ),
        (
            ".Bl -hang -width 3n\n.It Sy \"C\\zD\\zJ\" Em \"\\h'-3n'\\&\"\n.No \"B\" No \"Z\"\n.El",
            &["J   B Z"],
        ),
        (
            ".Bl -hang -width 3n\n.It Em \"C\\zD\\zJ\" Em \"\\h'-3n'\\&\"\n.No \"\" No \"X\"\n.El",
            &["J    X"],
        ),
        // DL-9 #2
        (
            ".Bl -inset\n.It Sy \"CDV\" Em \"\\h'-7n'\\~\"\n.No \"BV\"\n.Sy \"Z\"\n.El",
            &["C\u{a0} BV Z"],
        ),
        // Unprojected-blank & empty intermediate passes across HEAD/BODY
        (
            ".Bl -inset\n.It No \"C\\pJ\" Ns No \"\\h'-7n'\\&\"\n.No \"\\p\\:\"\n.Sy \"\\pZ\"\n.El",
            &["", "Z"],
        ),
        (
            ".Bl -inset\n.It Sy \"C\\p J\" Em \"\\h'-4n'\\&\"\n.No \"\\p\\c\\:\"\n.Sy \"\\pZ\"\n.El",
            &["C", "", "Z"],
        ),
    ]);
}

#[test]
fn cross_review_utf8_gaps_offset_containers_tabs_and_monotonic_anchors() {
    // Every expectation below was verified directly against pinned reference
    // mandoc (-Tutf8):
    // - utf8_getwidth('\b') == 0 and utf8_getwidth('\n') == 0
    //   (term_ascii.c:227-242), so open-head viscol stays in UTF-8 columns.
    // - Offset containers (.Bd -offset 4n, nested .Bl) compute body_units in
    //   the same container-relative frame as device.viscol.
    // - Tabs after omitted scalars rebase against NativeFieldDevice's omitted
    //   scalar ranges on both .br (hard_breaks.rs) and .mc (no_break.rs).
    // - Inset run-in overlap over a trailing breakable blank after \p tracks
    //   last_graph_position so the trailing U+00A0 row is not dropped.
    // - Rewound words with relocated earlier glyphs keep anchor.content
    //   monotonically non-decreasing across append_owner_intervals().
    // - CommittedField in no-fill mode suppresses duplicate EntryBlank
    //   projection, and retain_text() treats U+00A0 as printable ink.
    verify_rows(&[
        (r#".No "AB" No "\h'-3n'\p X""#, &["BX"]),
        (r#".No "AB" No "\h'-3n'\p X\p Y""#, &["BX", "Y"]),
        (r#".No "A\zB\~C""#, &["A\u{a0}C"]),
    ]);
    verify_definition_list_cases(&[
        // P1-1: UTF-8 utf8_getwidth('\b') == 0 gives 3-space gap (6 - 3 = 3)
        (
            ".Bl -hang -width 4n\n.It Sy \"\\zC\\pD\\zJ\" No \"\\:\\h'-6n'\\&\\p\\zF\" Em H Ns No \"\\h'-1n'X\"\n.No \"BODY\"\n.El",
            &["X   BODY"],
        ),
        // P1-2: offset container (.Bd -offset 4n) keeps 3-space native gap
        (
            ".Bd -offset 4n\n.Bl -hang -width 3n\n.It Sy \"C\\zD\\zJ\" Em \"\\h'-3n'\\&\"\n.No \"B\" No \"Z\"\n.El\n.Ed",
            &["J   B Z"],
        ),
        // P1-2: nested .Bl inside outer .Bl keeps 3-space native gap
        (
            ".Bl -tag -width 4n\n.It outer\n.Bl -hang -width 3n\n.It Sy \"C\\zD\\zJ\" Em \"\\h'-3n'\\&\"\n.No \"B\" No \"Z\"\n.El\n.El",
            &["outer", "", "J   B Z"],
        ),
        // P1-3: tabs after omitted scalars on .br-closed head
        (
            ".ta 10n\n.Bl -tag -width 12n\n.It A\\zBC\\tD\n.br\n.No X\n.El",
            &["AC        D", "X"],
        ),
        // P1-3: tabs after omitted scalars across .mc
        (
            ".ta 10n\n.Bl -tag -width 12n\n.It A\\zBC\\tD\n.mc |\n.No X\n.mc\n.El",
            &["AC        D   X"],
        ),
        // P1-4: inset trailing BreakableBlank overwritten to U+00A0 after \p
        (
            ".Bl -inset\n.It No \"C\\p  \" No \"\\h'-1n'\"\n.No \"\"\n.El\n.No AFTER",
            &["C", "", "AFTER"],
        ),
        // P2-6: .nf + .mc CommittedField with negative motion
        (
            ".Bl -tag -width 8n\n.It term\n.nf\n.No \"A.\"\n.mc |\n.No \"\\h'-1n'X\"\n.mc\n.fi\n.El",
            &["term", "A.", "X"],
        ),
    ]);
}
