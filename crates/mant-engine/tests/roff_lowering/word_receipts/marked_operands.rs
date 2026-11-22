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
