//! Native reading and portable Markdown use one IR execution result. These
//! exact Bx and Lk inputs were run with pristine CVS before the assertions.

use mant_codec::encode::{MarkdownOptions, render_addressable_markdown_with_options};
use mant_ir::ResolvedContent;
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};

const PRE: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

fn roundtrip(source: &str) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&query, false).unwrap();
    serde_json::from_str::<mant_protocol::QueryBundle>(&json)
        .unwrap()
        .into()
}

fn description_rows(markdown: &str) -> Vec<String> {
    let mut rows = Vec::new();
    let mut active = false;
    for event in pulldown_cmark::Parser::new(markdown) {
        match event {
            pulldown_cmark::Event::Text(text) if text.as_ref() == "DESCRIPTION" => active = true,
            pulldown_cmark::Event::Text(text) if text.as_ref() == "NEXT" => break,
            pulldown_cmark::Event::Text(text) | pulldown_cmark::Event::Code(text) if active => {
                if rows.is_empty() {
                    rows.push(String::new());
                }
                rows.last_mut().unwrap().push_str(&text);
            }
            pulldown_cmark::Event::HardBreak if active => rows.push(String::new()),
            pulldown_cmark::Event::SoftBreak if active => rows.last_mut().unwrap().push(' '),
            _ => {}
        }
    }
    rows
}

fn search(
    query: &ResolvedContent,
    scope: SearchScope,
    pattern: &str,
) -> mant_protocol::QuerySearch {
    mant_query::search_query(
        query,
        &SearchQuery {
            pattern: pattern.into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .unwrap()
}

#[test]
fn bsd_lifecycle_operands_keep_source_spacing_and_hard_rows() {
    // post_bx adds the native BSD word tightly; term_word/term_fill consume
    // source p/c/z in that same execution stream. Default Markdown exports
    // those same accepted glyphs without replacement English prose.
    for (before, operand, native_rows) in [
        ("BEFORE", "-alpha", vec!["BEFORE -alphaBSD AFTER"]),
        (r"BEFORE\p", "-alpha", vec!["BEFORE", "-alphaBSD AFTER"]),
        ("BEFORE", r"-alpha\p", vec!["BEFORE -alphaBSD", "AFTER"]),
        (
            r"BEFORE\p",
            r"-alpha\p",
            vec!["BEFORE", "-alphaBSD", "AFTER"],
        ),
        (r"BEFORE\c", "-alpha", vec!["BEFORE-alphaBSD AFTER"]),
        (r"BEFORE\c", r"-alpha\p", vec!["BEFORE-alphaBSD", "AFTER"]),
    ] {
        let source = format!("{PRE}.No {before}\n.Bx {operand}\n.No AFTER\n.Sh NEXT\n.No END\n");
        let query = roundtrip(&source);
        assert_eq!(
            description_rows(&mant_codec::encode::render_markdown(&query)),
            native_rows,
            "{before}/{operand}: portable"
        );
        let native = mant_codec::encode::render_markdown_with_options(
            &query,
            MarkdownOptions {
                native_text: true,
                ..MarkdownOptions::default()
            },
        );
        assert_eq!(
            description_rows(&native),
            native_rows,
            "{before}/{operand}: native"
        );
        let terminal = mant_render::render_query_man(&query);
        let rows: Vec<_> = terminal.lines().map(str::trim_start).collect();
        let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
        let end = rows.iter().position(|row| *row == "NEXT").unwrap();
        assert_eq!(
            &rows[start..end - 1],
            native_rows,
            "{before}/{operand}: terminal"
        );
    }
}

#[test]
fn accepted_link_suffix_keeps_its_executed_break_in_markdown() {
    // termp_lk_pre emits label -> ':' -> address. A label-final p stays
    // pending through ':' and ends the row at the address word boundary.
    let query = roundtrip(&format!(
        "{PRE}.Lk https://e.example/x \"label\\p\"\n.No AFTER\n.Sh NEXT\n.No END\n"
    ));
    assert_eq!(
        description_rows(&mant_codec::encode::render_markdown(&query)),
        ["label:", "https://e.example/x AFTER"]
    );
    assert_eq!(
        description_rows(&mant_codec::encode::render_markdown_with_options(
            &query,
            MarkdownOptions {
                native_text: true,
                ..MarkdownOptions::default()
            }
        )),
        ["label:", "https://e.example/x AFTER"]
    );
}

#[test]
fn visible_and_markdown_search_coordinates_use_the_only_spelling() {
    let query = roundtrip(&format!(
        "{PRE}.No BEFORE\\p\n.Bx -alpha\n.No AFTER\n.Sh NEXT\n.No END\n"
    ));
    for (scope, found, absent, native_text) in [
        (
            SearchScope::Visible,
            "-alpha",
            "currently in alpha test",
            true,
        ),
        (
            SearchScope::Markdown,
            "-alpha",
            "currently in alpha test",
            false,
        ),
    ] {
        let artifact = render_addressable_markdown_with_options(
            &query,
            MarkdownOptions {
                native_text,
                ..MarkdownOptions::ADDRESSABLE
            },
        );
        let found = search(&query, scope, found);
        assert_eq!(found.matches.len(), 1);
        assert_eq!(
            found.render.line_count,
            u32::try_from(artifact.text().lines().count()).unwrap()
        );
        let occurrence = &found.matches[0].occurrences[0];
        let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
            ..usize::try_from(occurrence.markdown.end_byte).unwrap();
        assert_eq!(&artifact.text()[range], occurrence.matched_text);
        assert_eq!(search(&query, scope, absent).matches.len(), 0);
    }
}
