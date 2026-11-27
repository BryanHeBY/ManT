use mant_codec::encode::{MarkdownOptions, render_markdown, render_markdown_with_options};
use mant_ir::{Block, Inline, LinkTarget, ResolvedContent, inline_plain_text};
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
        "+++",
        "++++",
        "+++ words",
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
fn literal_plus_rows_stay_out_of_metadata_on_repeated_export() {
    let label = "**[HEADING](https://example.test/target)**";
    for (authored, visible, hard_rows) in [
        (format!("+++<br>{label}"), "+++\nHEADING", 1),
        (format!("{label}<br>+++<br>tail"), "HEADING\n+++\ntail", 2),
        (format!("{label}<br>+++"), "HEADING\n+++", 1),
    ] {
        for level in [0, 1, 2] {
            for close in [false, true] {
                let first = match level {
                    1 => format!("# {authored}\n\n"),
                    2 => format!("# Probe\n\n## {authored}\n\n"),
                    _ => format!("# Probe\n\n{authored}\n\n"),
                };
                let source = format!(
                    "{first}BODY-ALPHA.\n\n{}BODY-BETA.\n\n+ List item.\n\n***\n",
                    if close { "+++\n\n" } else { "" },
                );
                for options in [MarkdownOptions::default(), MarkdownOptions::ADDRESSABLE] {
                    let mut content = load_markdown_text(&source, None).unwrap();
                    for round in 0..=2 {
                        assert_plus_row_owners(&content, level, visible, hard_rows, close, &source);
                        if round < 2 {
                            let exported = render_markdown_with_options(&content, options);
                            content = load_markdown_text(&exported, None)
                                .unwrap_or_else(|error| panic!("{source}\n{exported}\n{error}"));
                        }
                    }
                }
            }
        }
    }
}

fn assert_plus_row_owners(
    content: &ResolvedContent,
    level: usize,
    visible: &str,
    hard_rows: usize,
    close: bool,
    source: &str,
) {
    let document = content.document.as_ref().unwrap();
    let heading = document.heading.as_ref().expect(source);
    let root_blocks = body_blocks(&document.blocks).collect::<Vec<_>>();
    let section_blocks;
    let (inlines, blocks) = match (level, document.sections.as_slice(), root_blocks.as_slice()) {
        (1, [], body) => (&heading.content, body),
        (2, [section], []) => {
            section_blocks = body_blocks(&section.blocks).collect::<Vec<_>>();
            (&section.heading.content, section_blocks.as_slice())
        }
        (0, [], [Block::Paragraph { children, .. }, rest @ ..]) => (children, rest),
        _ => panic!("expected one designated owner for {source}: {document:#?}"),
    };
    assert_eq!(
        heading.plain_text(),
        if level == 1 { visible } else { "Probe" }
    );
    assert_eq!(inline_plain_text(inlines), visible, "{source}");
    assert_eq!(
        inlines
            .iter()
            .filter(|item| matches!(item, Inline::LineBreak { .. }))
            .count(),
        hard_rows,
        "{source}"
    );
    assert_eq!(
        super::links(document),
        [LinkTarget::External {
            uri: "https://example.test/target".to_owned()
        }],
        "{source}"
    );
    let [
        paragraphs @ ..,
        Block::List { items, .. },
        Block::ThematicBreak { .. },
    ] = blocks
    else {
        panic!("only the real list and rule belong to this owner: {source}\n{blocks:?}");
    };
    assert_eq!(items.len(), 1, "{source}");
    let paragraphs = paragraphs
        .iter()
        .map(|block| {
            let Block::Paragraph { children, .. } = block else {
                panic!("literal plus rows must remain paragraphs: {source}\n{block:?}");
            };
            inline_plain_text(children)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        paragraphs,
        if close {
            vec!["BODY-ALPHA.", "+++", "BODY-BETA."]
        } else {
            vec!["BODY-ALPHA.", "BODY-BETA."]
        },
        "{source}"
    );
}

fn body_blocks(blocks: &[Block]) -> impl Iterator<Item = &Block> {
    blocks.iter().filter(|block| {
        // Addressable Markdown writes raw HTML destinations. Their visible
        // source fallback on reimport is separate from the authored body.
        if let Block::Paragraph { children, .. } = block
            && let [Inline::Text { value }] = children.as_slice()
            && value.starts_with("<span id=\"")
            && value.ends_with("\"></span>")
        {
            return false;
        }
        !matches!(block, Block::VerticalSpace { .. })
    })
}

#[test]
fn authored_plus_metadata_is_still_read_as_an_unsupported_source_block() {
    let metadata = "+++\nkey = \"value\"\n+++\n";
    let source = format!("{metadata}\n# Probe\n\nBODY-SENTINEL.\n");
    let content = load_markdown_text(&source, None).unwrap();
    let document = content.document.as_ref().unwrap();
    let [
        Block::Unsupported { name, text, .. },
        Block::Paragraph { children, .. },
    ] = document.blocks.as_slice()
    else {
        panic!(
            "authored metadata and body have separate owners: {:?}",
            document.blocks
        );
    };
    assert_eq!(name.as_deref(), Some("metadata block"));
    assert_eq!(text.trim_end(), metadata.trim_end());
    assert_eq!(inline_plain_text(children), "BODY-SENTINEL.");
    assert_eq!(document.heading.as_ref().unwrap().plain_text(), "Probe");
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
