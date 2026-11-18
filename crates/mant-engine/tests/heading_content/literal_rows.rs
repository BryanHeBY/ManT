use mant_codec::encode::render_markdown;
use mant_ir::{Block, Inline, ResolvedContent, inline_plain_text};
use mant_loader::load_markdown_text;

#[test]
fn hard_rows_cannot_create_blocks_in_setext_headings_or_prose() {
    for row in [
        "-- --",
        "--- ---",
        "--\t--",
        "- - -",
        "---",
        "===",
        "-\twords",
        "+\twords",
        "1.\twords",
        "1)\twords",
        "- words",
        "+ words",
        "1. words",
        "1) words",
        "-",
        "+",
        "1.",
        "0)",
        "2.",
        "2)",
        "123456789.",
        "-- two words",
        "123",
        "1234567890.",
    ] {
        // Start with literal inline events even when the first row would
        // otherwise be a source-level list marker. Export must protect both
        // rows after expanding the authored hard break into a physical LF.
        let (first, rest) = row.split_at(1);
        let literal = format!("&#{};{rest}", first.as_bytes()[0]);
        for level in [None, Some(1), Some(2)] {
            for first_row in [false, true] {
                let (authored, expected) = if first_row {
                    (format!("{literal}<br>ordinary"), format!("{row}\nordinary"))
                } else {
                    (format!("ordinary<br>{literal}"), format!("ordinary\n{row}"))
                };
                let source = match level {
                    Some(1) => format!("# {authored}\n"),
                    Some(2) => format!("# Probe\n\n## {authored}\n"),
                    _ => format!("# Probe\n\n{authored}\n"),
                };
                let source = source + "\nBODY-SENTINEL.\n\n- Real list.\n\n***\n";
                let mut content = load_markdown_text(&source, None).unwrap();
                for round in 0..=2 {
                    let found = witness(&content, level);
                    assert_eq!(inline_plain_text(found), expected, "{source} round {round}");
                    assert_eq!(
                        found
                            .iter()
                            .filter(|inline| matches!(inline, Inline::LineBreak { .. }))
                            .count(),
                        1
                    );
                    if round < 2 {
                        let exported = render_markdown(&content);
                        content = load_markdown_text(&exported, None)
                            .unwrap_or_else(|error| panic!("{source}\n{exported}\n{error}"));
                    }
                }
            }
        }
    }
}

#[test]
fn deeper_atx_heading_folds_a_hard_row_without_losing_its_body() {
    let source = "# Probe\n\n## Parent\n\n### x<br>-- --\n\nBODY-SENTINEL.\n";
    let mut content = load_markdown_text(source, None).unwrap();
    for _ in 0..2 {
        let exported = render_markdown(&content);
        content = load_markdown_text(&exported, None).unwrap();
        let child = &content.document.as_ref().unwrap().sections[0].children[0];
        assert_eq!(child.heading.plain_text(), "x -- --");
        let [Block::Paragraph { children, .. }] = child.blocks.as_slice() else {
            panic!("the folded heading still owns its paragraph");
        };
        assert_eq!(inline_plain_text(children), "BODY-SENTINEL.");
    }
}

fn witness(content: &ResolvedContent, level: Option<u8>) -> &[Inline] {
    let document = content.document.as_ref().unwrap();
    let (title, blocks) = match level {
        Some(2) => {
            let [section] = document.sections.as_slice() else {
                panic!("the H2 and its body must survive");
            };
            assert_eq!(section.children.len(), 0);
            (
                Some(section.heading.content.as_slice()),
                section.blocks.as_slice(),
            )
        }
        Some(1) => {
            assert_eq!(document.sections.len(), 0);
            (
                Some(document.heading.as_ref().unwrap().content.as_slice()),
                document.blocks.as_slice(),
            )
        }
        _ => {
            assert_eq!(document.sections.len(), 0);
            (None, document.blocks.as_slice())
        }
    };
    let (witness, body) = if let Some(title) = title {
        (title, blocks)
    } else {
        let [Block::Paragraph { children, .. }, rest @ ..] = blocks else {
            panic!("the literal hard row must remain in a paragraph");
        };
        (children.as_slice(), rest)
    };
    let blocks = body
        .iter()
        .filter(|block| !matches!(block, Block::VerticalSpace { .. }))
        .collect::<Vec<_>>();
    let [
        Block::Paragraph { children, .. },
        Block::List { items, .. },
        Block::ThematicBreak { .. },
    ] = blocks.as_slice()
    else {
        panic!("only the real list and rule belong to this owner: {body:?}");
    };
    assert_eq!(inline_plain_text(children), "BODY-SENTINEL.");
    assert_eq!(items.len(), 1);
    witness
}
