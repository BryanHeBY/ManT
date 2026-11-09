//! Exact filled TEXT boundaries in a complete libxcb manual.

#[path = "support/fixtures.rs"]
#[allow(dead_code)]
mod common;
#[path = "support/json_roundtrip.rs"]
mod json_roundtrip;

use std::path::Path;

use libmandoc_rs::{MacroToken, ManMacro, Node, NodeKind, Parser};
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{Block, Inline, LinkTarget, ResolvedContent, inline_plain_text};

const FIXTURE: &str =
    "../../tests/fixtures/roff/real/source-lines/xcb_composite_unredirect_window.3.gz";
const FILLED_LINES: [(u32, &str, &str, &str); 3] = [
    (
        18,
        "window",
        "The window to terminate redirection of. Must be redirected by the",
        "current client, or a Value error results.",
    ),
    (
        21,
        "update",
        "The update type passed to RedirectWindows. If this does not match the",
        "previously requested update type, a Value error results.",
    ),
    (
        24,
        "",
        "Redirection of the specified window will be terminated. This cannot be",
        "used if the window was redirected with RedirectSubwindows.",
    ),
];

fn native_body(nodes: &[Node], macro_name: ManMacro, source_line: u32) -> &Node {
    nodes
        .iter()
        .find(|node| {
            node.kind == NodeKind::Block
                && node.line == source_line
                && node.macro_token == Some(MacroToken::Man(macro_name))
        })
        .unwrap_or_else(|| panic!("native {macro_name:?} block at line {source_line}"))
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Body)
        .expect("native BODY")
}

fn assert_consumer_rows(content: &ResolvedContent) {
    let rendered = mant_render::render_query_man_at_width(content, 160);
    for (_, _, before, after) in FILLED_LINES {
        assert_eq!(rendered.matches(after).count(), 1, "{rendered}");
        let row = rendered.lines().find(|row| row.contains(after)).unwrap();
        // Same physical row and exact visible words. TAB stops vary by consumer.
        let row = row.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
        assert!(row.ends_with(&format!("{before} {after}")), "{rendered}");
    }
}

#[test]
fn filled_tabs_preserve_real_description_and_argument_body_rows() {
    // Pristine man_term.c::print_man_node() only runs term_newln() here for
    // ASCII SP. The width-160 UTF-8 profile has three intact physical rows.
    // Tree and HTML establish two IP bodies and one SH paragraph.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    let report = Parser::default()
        .parse_file(&path)
        .expect("complete libxcb source");
    for (line, label, _, after) in FILLED_LINES {
        let body = if label.is_empty() {
            native_body(&report.document.root.children, ManMacro::Sh, 22)
        } else {
            let section = native_body(&report.document.root.children, ManMacro::Sh, 13);
            native_body(&section.children, ManMacro::Ip, line - 2)
        };
        let leaf = body.children.iter().find(|node| node.line == line).unwrap();
        assert_eq!(leaf.kind, NodeKind::Text);
        assert!(leaf.flags.line_start && !leaf.flags.no_fill);
        assert_eq!(
            leaf.native_text.as_deref().or(leaf.text.as_deref()),
            Some(format!("\t{after}").as_str()),
        );
    }

    let document = mant_loader::parse_manual_source(&path).expect("lower complete manual");
    let content = json_roundtrip::json_string_roundtrip(&common::query_for_document(
        "xcb_composite_unredirect_window",
        &document,
    ));
    let transported = content.document.as_ref().unwrap();
    let arguments =
        common::nested_definition_items(common::section(transported, "REQUEST ARGUMENTS"));
    assert_eq!(arguments.len(), 3);
    for (line, label, before, after) in FILLED_LINES {
        let owner = if label.is_empty() {
            &common::section(transported, "DESCRIPTION").blocks
        } else {
            let item = arguments
                .iter()
                .find(|item| {
                    item.entry
                        .as_ref()
                        .is_some_and(|entry| entry.names == [label])
                })
                .expect("lexical argument entry");
            assert_eq!(item.source.unwrap().line, line - 2);
            assert_eq!(inline_plain_text(&item.terms[0]), label);
            &item.description
        };
        let [
            Block::Paragraph {
                children, source, ..
            },
        ] = owner.as_slice()
        else {
            panic!("one paragraph in each original owner: {owner:?}");
        };
        assert_eq!(source.unwrap().line, line - 1);
        let text = inline_plain_text(children);
        assert!(
            !text.contains('\n'),
            "line {line} inserted a hard row: {text:?}"
        );
        assert!(
            text.contains('\t'),
            "raw TAB content was discarded: {text:?}"
        );
        assert_eq!(
            text.split_ascii_whitespace().collect::<Vec<_>>().join(" "),
            format!("{before} {after}")
        );
    }

    let [Block::Paragraph { children, .. }] =
        common::section(transported, "SEE ALSO").blocks.as_slice()
    else {
        panic!("native SEE ALSO paragraph");
    };
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: LinkTarget::Manual { name, manual_section }, children, .. }
            if name == "xcb-requests" && manual_section.as_deref() == Some("3")
                && inline_plain_text(children) == "xcb-requests(3)"
    )));

    assert_consumer_rows(&content);
    for options in [
        MarkdownOptions::default(),
        MarkdownOptions::READABLE_ADDRESSABLE,
    ] {
        let mut current = content.clone();
        for _ in 0..2 {
            let markdown = render_markdown_with_options(&current, options);
            current = mant_loader::load_markdown_text(&markdown, None).expect("Markdown readback");
            assert_consumer_rows(&current);
        }
    }
}
