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
            r#".Bl -inset
.It Em "A" No "\h'-7n'\p"
.No "\&"
.El"#,
            &[],
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
