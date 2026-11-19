use super::{MDOC, word_hits};

#[test]
fn a_blank_overstrike_edits_the_field_before_the_next_operand() {
    // term_word() writes the ordinary entry separator before the blank
    // overstrike. ESCAPE_OVERSTRIKE trims both slots. The next operand's
    // Ns or ordinary separator changes whether term_fill() accepts the
    // following row, and Pp explicitly starts a new field.
    // Pinned ASCII and UTF-8 agree for every source below.
    for prefix in ["PRE", r"\fBPRE\fR"] {
        for (boundary, retained) in [(" No ", false), (" Ns No ", true), ("\n.Pp\n.No ", false)] {
            let source = format!(
                "{MDOC}.No \"{prefix}\" No \"\\o' '\"{boundary}\"\\p NEEDLE\"\nTAILWORD\n.Pp\nENDTEXT\n"
            );
            let expected = if retained {
                ACCEPTS_SECOND_ROW
            } else {
                REJECTS_SUFFIX
            };
            assert_rows(&source, expected);
        }
    }
}

const ACCEPTS_SECOND_ROW: &[&[&str]] = &[&["PRE"], &["NEEDLE", "TAILWORD"], &["ENDTEXT"]];
const REJECTS_SUFFIX: &[&[&str]] = &[&["PRE"], &["ENDTEXT"]];

#[test]
fn adjacent_operands_reuse_the_proved_unowned_suffix() {
    // In term.c the glyph or blank drawing remains buffered on return
    // from term_word(). A later relative HORIZ can move within its slots,
    // and a bare BACKBEFORE from a blank overstrike can still consume the
    // new entry blank. These checks use the two fixed devices' exact rows.
    for (operands, expected) in [
        (
            r#".No "PRE" Ns No "\l'2n '" Ns No "\h'-1n'\p NEEDLE""#,
            REJECTS_SUFFIX,
        ),
        (
            r#".No "PRE" Ns No "\l'2n '" Ns No "\h'0n'\p NEEDLE""#,
            REJECTS_SUFFIX,
        ),
        (
            r#".No "PRE" No "\l'2n '" No "\o' '\p NEEDLE""#,
            REJECTS_SUFFIX,
        ),
        (
            r#".No "PRE" Ns No "\l'1n.'" No "\l'1n.'\p NEEDLE""#,
            ACCEPTS_SECOND_ROW,
        ),
        (
            r#".No "PRE\z" No "\o' '" No "\p NEEDLE""#,
            &[&["PRE", "NEEDLE"][..], &["TAILWORD"], &["ENDTEXT"]][..],
        ),
    ] {
        let source = format!("{MDOC}.ll 140n\n{operands}\nTAILWORD\n.Pp\nENDTEXT\n");
        assert_rows(&source, expected);
    }
}

#[test]
fn relative_motion_replays_the_buffered_prior_operand_before_the_break() {
    // term.c::term_word consumes a pending BACKBEFORE before applying its
    // negative HORIZ. The marker then overwrites a known older byte and
    // the next operand's blank closes the pass. The two device profiles
    // agree that NEEDLE and TAILWORD are on different rows.
    for (separator, label) in [("Ns No", "PRXNEEDLE"), ("No", "PREXNEEDLE")] {
        let source = format!(
            "{MDOC}.ll 140n\n.No \"PRE\\z\" {separator} \"X\" Ns No \"\\h'-1n'\\p NEEDLE\"\nTAILWORD\n.Pp\nENDTEXT\n"
        );
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let content = crate::json_roundtrip::json_string_roundtrip(&loaded);
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        for document in [&content, &readback] {
            let text = mant_render::render_query_text(document);
            let rows: Vec<_> = text
                .lines()
                .filter(|line| line.contains("NEEDLE") || line.contains("TAILWORD"))
                .collect();
            assert_eq!(rows, [label, "TAILWORD"], "{source}\n{text}");
            assert_eq!(word_hits(document, label), 1, "{source}\n{text}");
            assert_eq!(word_hits(document, "NEEDLE"), 0, "{source}\n{text}");
        }
    }
}

fn assert_rows(source: &str, expected: &[&[&str]]) {
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let content = crate::json_roundtrip::json_string_roundtrip(&loaded);
    let markdown = mant_codec::encode::render_markdown(&content);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for document in [&content, &readback] {
        let text = mant_render::render_query_text(document);
        assert_eq!(author_rows(&text), expected, "{source}\n{text}");
        for marker in ["NEEDLE", "TAILWORD", "ENDTEXT"] {
            assert_eq!(
                word_hits(document, marker),
                expected.iter().filter(|row| row.contains(&marker)).count(),
                "{source}\n{text}"
            );
        }
    }
}

fn author_rows(text: &str) -> Vec<Vec<&'static str>> {
    text.lines()
        .map(|row| {
            let mut words: Vec<_> = ["PRE", "NEEDLE", "TAILWORD", "ENDTEXT"]
                .into_iter()
                .flat_map(|word| row.match_indices(word).map(move |(index, _)| (index, word)))
                .collect();
            words.sort_unstable_by_key(|(index, _)| *index);
            words.into_iter().map(|(_, word)| word).collect::<Vec<_>>()
        })
        .filter(|words| !words.is_empty())
        .collect()
}
