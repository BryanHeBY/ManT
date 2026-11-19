use mant_ir::{Block, inline_plain_text};

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
