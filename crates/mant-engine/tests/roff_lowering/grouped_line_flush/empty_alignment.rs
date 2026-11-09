//! Empty alignment groups are hard rows independent of positive spacing.

use libmandoc_rs::{MacroToken, Node, NodeKind, Parser, RoffMacro};
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{Block, inline_plain_text};

use super::{description, json_string_roundtrip, load};

const MINIMUM: &str = include_str!("empty-alignment.1");
const MAN: &str = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH DESCRIPTION\n";

fn alignment(node: &Node) -> Option<&Node> {
    if matches!(
        node.macro_token,
        Some(MacroToken::Roff(RoffMacro::Ce | RoffMacro::Rj))
    ) {
        Some(node)
    } else {
        node.children.iter().find_map(alignment)
    }
}

fn assert_group_payload(source: &str, text_children: usize, request_children: usize) {
    let parsed = Parser::default()
        .parse_bytes("empty-alignment.1", source.as_bytes())
        .unwrap();
    let grouped = alignment(&parsed.document.root).unwrap();
    let payload = &grouped.children[1..];
    let texts = payload
        .iter()
        .filter(|node| node.kind == NodeKind::Text)
        .count();
    assert_eq!(
        (texts, payload.len() - texts),
        (text_children, request_children),
        "{source}"
    );
}

fn assert_paragraph_contributions(
    source: &str,
    blocks: &[Block],
    breaks: usize,
    explicit_gap: u16,
) {
    let body_breaks = blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_plain_text(children).matches('\n').count(),
            _ => 0,
        })
        .sum::<usize>();
    let gaps = blocks
        .iter()
        .map(|block| match block {
            Block::VerticalSpace { lines, .. } => *lines,
            _ => 0,
        })
        .sum::<u16>();
    assert_eq!((body_breaks, gaps), (breaks, explicit_gap), "{source}");
    let Some(Block::Paragraph { layout, .. }) = blocks.last() else {
        panic!("expected PP paragraph: {blocks:?}");
    };
    assert_eq!(layout.spacing_before_lines, 1, "{source}");
}

#[test]
fn completed_alignment_rows_stay_distinct_from_paragraph_gaps() {
    // The exact inputs were replayed with pinned CVS before these
    // expectations. roff_term_pre_ce() (roff_term.c:81–107) partitions
    // children at the next NODE_LINE TEXT and calls term_flushln() after
    // each group. It does not require a visible glyph or even a TEXT
    // child. term_vspace() (term.c:489–497) accounts for the separate PP
    // and sp distances.
    for (source, expected, breaks, explicit_gap, text_children, request_children) in [
        (MINIMUM.to_owned(), "BEFOREMARK\n\n\nAFTERMARK", 2, 0, 1, 0),
        (
            format!("{MAN}BEFOREMARK\n.ce 1\n.PP\nAFTERMARK\n"),
            "BEFOREMARK\n\nAFTERMARK",
            0,
            0,
            0,
            0,
        ),
        (
            format!("{MAN}BEFOREMARK\n.ce 1\n.ft B\n.ft R\n.PP\nAFTERMARK\n"),
            "BEFOREMARK\n\n\nAFTERMARK",
            2,
            0,
            0,
            2,
        ),
        (
            format!("{MAN}BEFOREMARK\n.ce 1\n.br\n.PP\nAFTERMARK\n"),
            "BEFOREMARK\n\n\nAFTERMARK",
            2,
            0,
            0,
            1,
        ),
        (
            format!("{MAN}BEFOREMARK\n.ce 1\n.sp 1\n\\z\n.PP\nAFTERMARK\n"),
            "BEFOREMARK\n\n\n\n\nAFTERMARK",
            2,
            1,
            1,
            1,
        ),
        (
            format!("{MAN}BEFOREMARK\n.ce 1\n\\z\n.sp 1\n.PP\nAFTERMARK\n"),
            "BEFOREMARK\n\n\n\nAFTERMARK",
            1,
            1,
            1,
            1,
        ),
        (
            format!("{MAN}BEFOREMARK\n.rj 1\n\\&\n.PP\nAFTERMARK\n"),
            "BEFOREMARK\n\n\nAFTERMARK",
            2,
            0,
            1,
            0,
        ),
        (
            format!("{MAN}.ce 1\n\\z\n.PP\nAFTERMARK\n"),
            "\n\nAFTERMARK",
            1,
            0,
            1,
            0,
        ),
    ] {
        assert_group_payload(&source, text_children, request_children);
        let original = json_string_roundtrip(&load(&source));
        assert_paragraph_contributions(
            &source,
            &original.document.as_ref().unwrap().sections[1].blocks,
            breaks,
            explicit_gap,
        );
        assert_eq!(
            description(&mant_render::render_query_man(&original)),
            expected
        );

        for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
            let mut content = original.clone();
            for round in 1..=2 {
                let markdown = render_markdown_with_options(&content, options);
                content = json_string_roundtrip(
                    &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                );
                let rendered = mant_render::render_query_text(&content);
                assert_eq!(
                    description(&rendered),
                    expected,
                    "{options:?} cycle {round}: {source}"
                );
            }
        }
    }
}
