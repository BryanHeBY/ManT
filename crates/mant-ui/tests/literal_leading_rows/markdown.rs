//! Actual Markdown import/export retains this policy's already resolved rows.
use crate::{Block, ResolvedContent, Value, body_rows, literal_texts, plain_rows, restored};
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};

pub(crate) fn assert_roff_readback(case: &Value, content: &ResolvedContent, expected: &[String]) {
    let markdown = render_markdown_with_options(content, MarkdownOptions::default());
    let read =
        mant_loader::load_markdown_text(&markdown, Some("literal-export.md".into())).unwrap();
    if case["native"]["bodyRows"]["utf8"]
        .as_array()
        .unwrap()
        .is_empty()
    {
        // With no executed literal row there is no code owner. Two ordinary
        // Markdown paragraphs require their own syntax separator; that known
        // paragraph geometry policy is outside the literal reading exception.
        assert!(body_rows(&plain_rows(&read)).iter().all(String::is_empty));
    } else {
        assert_eq!(
            body_rows(&plain_rows(&read)),
            expected,
            "{} Markdown\n{markdown}",
            case["id"]
        );
    }
    assert_eq!(
        literal_texts(&read).as_slice(),
        literal_texts(content),
        "{} literal payload\n{markdown}",
        case["id"]
    );
}

#[test]
fn markdown_authored_fences_keep_zero_one_two_and_whitespace_initial_rows() {
    for prefix in ["", "\n", "\n\n", "  \n", "\u{a0}\n"] {
        let source =
            format!("# Markdown\n\nBEFORE\n\n```sh\n{prefix}LINE_A\n\nLINE_B\n\n```\n\nAFTER\n");
        let loaded = mant_loader::load_markdown_text(&source, None).unwrap();
        let content = restored(&loaded);
        let expected = format!("{prefix}LINE_A\n\nLINE_B\n");
        assert_eq!(
            literal_texts(&content).as_slice(),
            std::slice::from_ref(&expected),
            "{source}"
        );
        let mut current = content;
        for _ in 0..2 {
            let markdown = render_markdown_with_options(&current, MarkdownOptions::default());
            let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
            assert_eq!(
                literal_texts(&read).as_slice(),
                std::slice::from_ref(&expected),
                "{markdown}"
            );
            let document = read.document.as_ref().unwrap();
            assert!(
                document
                    .blocks
                    .iter()
                    .all(|block| !matches!(block, Block::Unsupported { .. }))
            );
            current = read;
        }
    }
}
