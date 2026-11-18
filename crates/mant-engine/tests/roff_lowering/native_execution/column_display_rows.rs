//! A column's executed row boundary survives without a visible glyph.

use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{Block, DocumentIndex, NodeId, ResolvedContent, TableRow};

mod geometry;

const HEADER: &str =
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n";
const FINISH: &str = ".El\n.Sh NEXT\n.No SKIP\n";

fn lower(source: &str) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    crate::json_roundtrip::json_string_roundtrip(&query)
}

fn table(query: &ResolvedContent) -> &[TableRow] {
    let [Block::Table { rows, .. }] = query.document.as_ref().unwrap().sections[1]
        .blocks
        .as_slice()
    else {
        panic!("expected the authored column table");
    };
    rows
}

fn word_rows(payload: &str) -> Vec<String> {
    payload
        .lines()
        .map(|row| {
            row.split_ascii_whitespace()
                .filter(|word| *word != "|")
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

fn reading_body(rendered: &str) -> &str {
    let (_, rest) = rendered.split_once("\nDESCRIPTION\n").unwrap();
    rest.split_once("\n\nNEXT\n").unwrap().0
}

fn markdown_rows(query: &ResolvedContent) -> Vec<String> {
    let markdown = render_markdown_with_options(query, MarkdownOptions::default());
    let decoded = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let [Block::Preformatted { children, .. }] = decoded.document.as_ref().unwrap().sections[1]
        .blocks
        .as_slice()
    else {
        panic!("one portable table fence: {markdown}");
    };
    word_rows(&mant_ir::inline_plain_text(children))
}

fn check_pair(row: &str, display: &str, next_item: &str, slot: usize, expected: &[&str]) {
    let mut prior = None;
    for tag in ["", ".Tg Mixed.Target\n"] {
        let source =
            format!("{HEADER}.Bl -column aa bb cc\n{row}{tag}{display}{next_item}{FINISH}");
        let query = lower(&source);
        let first = &table(&query)[0].cells;
        assert_eq!(
            first[slot].break_after,
            slot < 2 && !display.starts_with(".No"),
            "{source}"
        );
        let rendered = mant_render::render_query_text(&query);
        assert_eq!(word_rows(reading_body(&rendered)), expected, "{source}");
        let fences = markdown_rows(&query);
        assert_eq!(fences, expected, "{source}");
        let flags = first
            .iter()
            .map(|cell| cell.break_after)
            .collect::<Vec<_>>();
        let index = DocumentIndex::build(query.document.as_ref().unwrap());
        let addressable = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
        if let Some((plain, plain_fences, plain_flags)) = &prior {
            assert_eq!(
                (&rendered, &fences, &flags),
                (plain, plain_fences, plain_flags),
                "{source}"
            );
            assert_eq!(
                index.fragment_target("Mixed.Target").map(NodeId::as_str),
                Some("mixed-target")
            );
            assert_eq!(
                addressable.matches("<a id=\"Mixed.Target\"></a>").count(),
                1
            );
        } else {
            assert!(index.fragment_target("Mixed.Target").is_none());
            assert!(!addressable.contains("<a id=\"Mixed.Target\"></a>"));
            prior = Some((rendered, fences, flags));
        }
    }
}

#[test]
fn display_column_tail_is_independent_of_its_navigation_anchor() {
    // All 34 complete inputs ran the fixed CVS ASCII, UTF-8, HTML, tree
    // and lint profiles. termp_d1_pre/Bd pre can close an earlier field.
    // Display post term_newln and It BODY post's term_flushln have distinct
    // row effects (mdoc_term.c:953,1148,1329,1482; term.c:250-253).
    for (middle, last, empty_rows) in [
        (".D1 \\& Ta THIRD\n", ".D1 \\&\n", 2),
        (".Dl \\& Ta THIRD\n", ".Dl \\&\n", 2),
        (
            ".Bd -literal\n\\&\n.Ed\n.Ta THIRD\n",
            ".Bd -literal\n\\&\n.Ed\n",
            3,
        ),
    ] {
        check_pair(".It FIRST Ta \\&\n", middle, "", 1, &["FIRST", "THIRD"]);
        check_pair(".It FIRST Ta PRE\n", middle, "", 1, &["FIRST PRE", "THIRD"]);
        check_pair(".It \\&\n", middle, "", 0, &["", "THIRD"]);
        let mut expected = vec!["FIRST"];
        expected.extend(std::iter::repeat_n("", empty_rows));
        expected.push("AFTER END LAST");
        check_pair(
            ".It FIRST Ta \\& Ta \\&\n",
            last,
            ".It AFTER Ta END Ta LAST\n",
            2,
            &expected,
        );
    }
    for macro_name in ["D1", "Dl"] {
        check_pair(
            ".It FIRST Ta \\&\n",
            &format!(".{macro_name} OUT Ta THIRD\n"),
            "",
            1,
            &["FIRST OUT", "THIRD"],
        );
        check_pair(
            ".It FIRST Ta \\& Ta \\&\n",
            &format!(".{macro_name} OUT\n"),
            ".It AFTER Ta END Ta LAST\n",
            2,
            &["FIRST", "OUT", "", "AFTER END LAST"],
        );
    }
    check_pair(
        ".It FIRST Ta \\&\n",
        ".No \\& Ta THIRD\n",
        "",
        1,
        &["FIRST THIRD"],
    );
}

#[test]
fn empty_display_leaves_the_authorized_column_origins_unchanged() {
    for tag in ["", ".Tg Mixed.Target\n"] {
        let source = format!(
            "{HEADER}.Bl -column aa bb cc\n.It FIRST Ta \\&\n{tag}.D1 \\& Ta THIRD\n{FINISH}"
        );
        let query = lower(&source);
        let text = mant_render::render_query_text(&query);
        let body = reading_body(&text);
        let (first, third) = body.split_once('\n').unwrap();
        assert_eq!(first.find("FIRST"), Some(0));
        assert_eq!(third.find("THIRD"), Some(12));
        assert!(table(&query)[0].cells[1].break_after);
    }
}
