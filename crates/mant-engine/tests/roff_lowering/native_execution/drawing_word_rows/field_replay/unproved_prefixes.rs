use super::OwnedLabels;
use mant_ir::{Block, inline_plain_text, visit::Visit};

const HEADER: &str = concat!(
    ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n",
    ".Nm probe\n.Nd test page\n.Sh DESCRIPTION\n",
);

#[test]
fn ambiguous_prior_slot_positions_respect_buffered_and_unwritten_zero_advance() {
    // Fixed CVS term.c::encode1 adds decoration bytes, and term_word can
    // then address the earlier operand with absolute HORIZ. Both devices
    // retain XYZ after a buffered glyph in these exact styled and long
    // prefixes. A bare BACKAFTER still waits for the first drawing byte;
    // the same absolute edit then leaves the suffix outside the accepted pass.
    for (line_length, prior, retained) in [
        ("", r"\fBA\fP\zX".to_owned(), 1),
        (".ll 180n\n", format!("{}\\zX", "A".repeat(63)), 1),
        (".ll 180n\n", format!("{}\\zX", "A".repeat(65)), 1),
        (".ll 180n\n", format!("\\fB{}\\fP\\z", "A".repeat(63)), 0),
        (".ll 180n\n", format!("\\fB{}\\fP\\z", "A".repeat(65)), 0),
    ] {
        let source = format!(
            "{HEADER}{line_length}.Bl -tag -width 4n\n.It Xo\n.No {prior}\n.No \"\\l'2n '\\h'|2n'\\p XYZ\"\n.Xc\nENDING\n.El\n"
        );
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let content = crate::json_roundtrip::json_string_roundtrip(&loaded);
        let [Block::DefinitionList { items, .. }] = content.document.as_ref().unwrap().sections[1]
            .blocks
            .as_slice()
        else {
            panic!("{source}");
        };
        let [item] = items.as_slice() else {
            panic!("{source}");
        };
        assert_eq!(
            inline_plain_text(&item.terms[0]).matches("XYZ").count(),
            retained,
            "{source}"
        );
        let [Block::Paragraph { children, .. }] = item.description.as_slice() else {
            panic!("{source}");
        };
        assert_eq!(inline_plain_text(children), "ENDING", "{source}");
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        let text = mant_render::render_query_text(&readback);
        assert_eq!(
            text.matches("XYZ").count(),
            retained,
            "{source}\n{markdown}\n{text}"
        );
    }
}

#[test]
fn armed_blank_overstrike_updates_only_its_local_word_entry() {
    // term.c::term_word adds the automatic separator before escape
    // execution. ESCAPE_OVERSTRIKE trims that separator and its own
    // blank. An empty operand supplies no new cell, leaving BACKAFTER to
    // cover the X of XYZ. Both fixed devices agree in each list style.
    for style in ["tag -width 4n", "hang -width 4n", "inset"] {
        for (operand, label) in [(" ", "AXYZ"), ("", "A\nYZ")] {
            let source = format!(
                "{HEADER}.Bl -{style}\n.It Xo\n.No \\fBA\\fP\\z\n.No \"\\o'{operand}'\\p XYZ\"\n.Xc\nENDING\n.El\n"
            );
            let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let content = crate::json_roundtrip::json_string_roundtrip(&loaded);
            let [Block::DefinitionList { items, .. }] = content.document.as_ref().unwrap().sections
                [1]
            .blocks
            .as_slice() else {
                panic!("{source}");
            };
            let [item] = items.as_slice() else {
                panic!("{source}");
            };
            assert_eq!(inline_plain_text(&item.terms[0]), label, "{source}");
            let [Block::Paragraph { children, .. }] = item.description.as_slice() else {
                panic!("{source}");
            };
            assert_eq!(
                inline_plain_text(children).trim_start_matches(' '),
                "ENDING",
                "{source}"
            );
            let markdown = mant_codec::encode::render_markdown(&content);
            let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
            let text = mant_render::render_query_text(&readback);
            assert_eq!(
                text.matches("AXYZ").count(),
                usize::from(operand == " "),
                "{source}\n{markdown}\n{text}"
            );
            assert_eq!(
                text.matches("YZ").count(),
                1,
                "{source}\n{markdown}\n{text}"
            );
        }
    }
}

#[test]
fn absolute_motion_around_a_local_overstrike_keeps_unproved_author_words() {
    // term_word() executes the blank-pair trim and absolute HORIZ in source
    // order. HORIZ consumes an armed BACKAFTER, but once the overstrike runs
    // first, HORIZ needs col from before this word (term.c:678–714, 776–796).
    // Both fixed devices print XYZ in all three orders below.
    for style in ["tag -width 4n", "hang -width 4n", "inset"] {
        for edit in [r"\o' '\h'|3n'", r"\o''\h'|3n'", r"\h'|3n'\o' '"] {
            let source = format!(
                "{HEADER}.ll 180n\n.Bl -{style}\n.It Xo\n.No \\fBA\\fP\\z\n.No \"{edit}\\p XYZ\"\n.Xc\nENDING\n.El\n"
            );
            let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let content = crate::json_roundtrip::json_string_roundtrip(&loaded);
            let [Block::DefinitionList { items, .. }] = content.document.as_ref().unwrap().sections
                [1]
            .blocks
            .as_slice() else {
                panic!("{source}");
            };
            let [item] = items.as_slice() else {
                panic!("{source}");
            };
            assert_eq!(
                inline_plain_text(&item.terms[0]).matches("XYZ").count(),
                1,
                "{source}"
            );
            let [Block::Paragraph { children, .. }] = item.description.as_slice() else {
                panic!("{source}");
            };
            assert_eq!(
                inline_plain_text(children).trim_start_matches(' '),
                "ENDING",
                "{source}"
            );
            let markdown = mant_codec::encode::render_markdown(&content);
            let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
            let text = mant_render::render_query_text(&readback);
            assert_eq!(
                text.matches("XYZ").count(),
                1,
                "{source}\n{markdown}\n{text}"
            );
        }
    }
}

#[test]
fn a_bounded_slot_replay_cannot_split_an_unresolved_overstrike() {
    // Fixed ASCII and UTF-8 both write A, backspace, B, C, backspace,
    // and D. The later trim must keep the older coverage observation even
    // when the complete C/D pair lies within the retained suffix.
    for length in [56, 57, 58, 59] {
        let letters = "q".repeat(length);
        for second in ["", ".No \"\\o' 'END\"\n"] {
            let source = format!(
                "{HEADER}.Bl -hang -width 75n\n.It Xo\n.No \"\\zA\\:B\\o'CD'{letters}\"\n{second}.Xc\n.No BODY\n.El\n"
            );
            let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let content = crate::json_roundtrip::json_string_roundtrip(&loaded);
            let [Block::DefinitionList { items, .. }] = content.document.as_ref().unwrap().sections
                [1]
            .blocks
            .as_slice() else {
                panic!("{source}");
            };
            let ending = if second.is_empty() { "" } else { "END" };
            assert_eq!(
                inline_plain_text(&items[0].terms[0]),
                format!("BD{letters}{ending}"),
                "{source}"
            );
        }
    }
}

#[test]
fn field_edits_keep_the_complete_short_prefix_around_styled_operands() {
    // Exact pinned ASCII and UTF-8 agree on the remaining nonempty rows.
    // termp_in_pre/post and termp_lk_pre place regular punctuation around
    // an underlined operand. encode1()'s underline and backspace occupy
    // raw slots, so a rewind can cut the prefix, the glyph or its suffix.
    // termp_lk_pre() writes the target as a separate word. A surviving
    // separator then realizes a buffered \p in term_fill()/term_flushln().
    let cases: &[(&str, [&[&str]; 4])] = &[
        ("Ar X", [&["X"], &[], &[], &[]]),
        ("In x", [&["x>"], &["x>"], &[], &[]]),
        ("Lk u t", [&["t:", "u"], &[":", "u"], &["u"], &[]]),
        ("Fn x", [&["x()"], &["()"], &[], &[]]),
        (r"Xr \& \&", [&["()"], &[")"], &[], &[]]),
    ];
    for &(operand, expected) in cases {
        for motion in [r"\h'|0n'", r"\h'-12n'"] {
            for (count, expected) in [1, 3, 5, 12].into_iter().zip(expected) {
                let word = format!(r#"{operand} Ns No "{motion}{}""#, r"\p".repeat(count));
                for display in [
                    format!(".Dl {word}"),
                    format!(".Bd -literal -compact\n.{word}\n.Ed"),
                ] {
                    let source = format!(
                        "{HEADER}STARTPROBE\n.br\n{display}\n.br\nSTOPPROBE\n.Sh ENDPROBE\nDone.\n"
                    );
                    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
                    let restored = crate::json_roundtrip::json_string_roundtrip(&loaded);
                    if operand == "Lk u t" {
                        let mut labels = OwnedLabels::default();
                        labels.visit_document(restored.document.as_ref().unwrap());
                        let label = match count {
                            1 => "t",
                            3 | 5 => "u",
                            _ => "",
                        };
                        assert_eq!(labels.0, [("u".into(), label.into())], "{source}");
                    }
                    let markdown = mant_codec::encode::render_markdown(&restored);
                    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
                    for content in [&restored, &readback] {
                        let text = mant_render::render_query_text(content);
                        let (_, tail) = text.split_once("STARTPROBE\n").unwrap();
                        let (region, _) = tail.split_once("STOPPROBE").unwrap();
                        let rows: Vec<_> = region
                            .lines()
                            .map(str::trim)
                            .filter(|line| !line.is_empty())
                            .collect();
                        assert_eq!(rows, expected, "{source}\n{text}");
                    }
                }
            }
        }
    }
}
