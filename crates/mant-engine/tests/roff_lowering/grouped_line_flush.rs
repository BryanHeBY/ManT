//! Physical line and native word state across grouped and conditional flushes.

use libmandoc_rs::{MacroToken, Node, NodeKind, Parser, RoffMacro};
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{Block, ResolvedContent, inline_plain_text};
use serde::Deserialize;

use super::json_roundtrip::json_string_roundtrip;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    oracle_sha256: String,
    profiles_per_source: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    id: String,
    source: String,
    native_rows: Vec<String>,
    #[serde(default)]
    omit_single_initial_code_row: bool,
}

fn description(text: &str) -> &str {
    text.split_once("\nDESCRIPTION\n").unwrap().1
}

#[test]
fn grouped_flushes_keep_native_rows_and_ordinary_newlines_keep_their_guard() {
    // All sources ran pristine ASCII, UTF-8 at 160 columns, HTML, tree and
    // lint first. roff_term_pre_ce() directly executes term_word and roff
    // requests until the next NODE_LINE TEXT. It calls term_flushln() for
    // every group, even with no cells; term_newln() alone has a buffer guard
    // (roff_term.c:81-107, term.c:233-253,475-481).
    let fixture: Fixture =
        serde_json::from_str(include_str!("grouped_line_flush/cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.profiles_per_source, 5);
    assert_eq!(fixture.cases.len(), 1230);
    let mut reading_differences = 0;
    for case in fixture.cases {
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap());
        let rendered = mant_render::render_query_man(&content);
        let actual = description(&rendered)
            .lines()
            // Native centering/right alignment and device TAB expansion are
            // not the same columns as the portable projection. Compare
            // every physical row and preserve all interior word separators.
            // The focused fixture below checks authored whitespace exactly.
            .map(|row| row.trim_matches([' ', '\t']))
            .collect::<Vec<_>>();
        let expected = if case.omit_single_initial_code_row {
            // Public reading strips one initial blank in an otherwise
            // substantive code block. Multiple authored initial rows and
            // every interior or trailing row stay in the matrix.
            assert_eq!(case.native_rows, ["", "NEXTMARK", "ENDMARK"]);
            reading_differences += 1;
            &case.native_rows[1..]
        } else {
            &case.native_rows[..]
        };
        assert_eq!(actual, expected, "{}: {}", case.id, case.source);
    }
    assert_eq!(reading_differences, 2);
}

fn find_center(node: &Node) -> Option<&Node> {
    if node.macro_token == Some(MacroToken::Roff(RoffMacro::Ce)) {
        Some(node)
    } else {
        node.children.iter().find_map(find_center)
    }
}

fn load(source: &str) -> ResolvedContent {
    json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap())
}

#[test]
fn grouped_literal_tab_keeps_one_owned_row_through_markdown() {
    // The exact source was run with pristine UTF-8/ASCII/tree/HTML/lint.
    // term_field() delays native padding without printing it
    // (term.c:389-427); ce closes that row once. The source TAB remains
    // literal content and must not also create an empty layout row.
    let source = include_str!("grouped_line_flush/literal-tab.1");
    let parsed = Parser::default()
        .parse_bytes("literal-tab.1", source.as_bytes())
        .unwrap();
    let center = find_center(&parsed.document.root).expect("native grouped request");
    let [count, tab, next] = center.children.as_slice() else {
        panic!("expected two direct grouped TEXT operands: {center:?}");
    };
    assert_eq!(count.text.as_deref(), Some("2"));
    assert_eq!(tab.kind, NodeKind::Text);
    assert!(tab.flags.line_start && tab.flags.no_fill);
    assert_eq!(tab.text.as_deref(), Some("\t"));
    assert_eq!(next.text.as_deref(), Some(" NEXT"));
    assert!(next.flags.line_start && next.flags.no_fill);
    let original = load(source);
    let document = original.document.as_ref().unwrap();
    let section = document
        .sections
        .iter()
        .find(|section| inline_plain_text(&section.heading.content) == "DESCRIPTION")
        .unwrap();
    let [Block::Preformatted { children, .. }] = section.blocks.as_slice() else {
        panic!("expected one literal owner: {:?}", section.blocks);
    };
    assert_eq!(inline_plain_text(children), "BEFORE\n\t\n NEXT\nAFTER");
    for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
        let mut content = original.clone();
        for _ in 0..=2 {
            let plain = mant_render::render_query_text(&content);
            let ansi = mant_render::render_query_text_with(&content, |_, text| {
                format!("\x1b[1m{text}\x1b[0m")
            });
            assert_eq!(description(&plain), "BEFORE\n\t\n NEXT\nAFTER");
            assert_eq!(ansi.replace("\x1b[1m", "").replace("\x1b[0m", ""), plain);
            let markdown = render_markdown_with_options(&content, options);
            content =
                json_string_roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
        }
    }
}

#[test]
fn grouped_hard_rows_survive_both_markdown_profiles() {
    // These exact sources are from the pristine tree and text receipts.
    // ce dispatches the same unconditional flush in a bracket enclosure;
    // an empty .br leaves BACKAFTER armed and consumes the N in NEXT.
    // roff_term_pre_ce ends rows, while pre_PP's vertical space starts a
    // real paragraph. Both boundaries must survive portable Markdown.
    let centered = include_str!("grouped_line_flush/bare-zero-advance-group.1");
    for (source, expected_body) in [
        (centered.to_owned(), "BEFORE\n\n NEXT\nAFTER"),
        (centered.replace(".ce 2", ".rj 2"), "BEFORE\n\n NEXT\nAFTER"),
        (
            centered.replace("\nAFTER\n", "\n.PP\nAFTER\n"),
            "BEFORE\n\n NEXT\n\nAFTER",
        ),
        (
            centered.replace(".ce 2\n\\z\n NEXT\n", ".ce 0\nNEXT\n"),
            "BEFORE\nNEXT AFTER",
        ),
        (
            include_str!("grouped_line_flush/bare-zero-advance-enclosure.1").to_owned(),
            "BEFORE [\n\n NEXT\n] AFTER",
        ),
        (
            include_str!("grouped_line_flush/conditional-break.1").to_owned(),
            "BEFORE\nEXT AFTER",
        ),
    ] {
        let original = load(&source);
        assert_eq!(
            description(&mant_render::render_query_man(&original)),
            expected_body
        );
        for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
            let mut content = original.clone();
            for _ in 0..2 {
                let markdown = render_markdown_with_options(&content, options);
                content = json_string_roundtrip(
                    &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                );
                let rendered = mant_render::render_query_text(&content);
                let body = description(&rendered);
                assert_eq!(body, expected_body, "{source}");
            }
        }
    }
}

#[path = "grouped_line_flush/hanging_alignment.rs"]
mod hanging_alignment;

#[path = "grouped_line_flush/empty_alignment.rs"]
mod empty_alignment;
