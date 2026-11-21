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
