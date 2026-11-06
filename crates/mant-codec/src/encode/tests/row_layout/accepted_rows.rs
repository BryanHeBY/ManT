//! Accepted code fragments retain the original owner's logical row identity.
use super::super::{
    Block, Document, Inline, LayoutHint, MarkdownFragmentOptions, MarkdownOptions, TableCell,
    TableRow, parse_content, render_blocks_fragment, render_markdown_with_options,
};
use crate::encode::render_addressable_markdown_with_options;
use mant_ir::{InlineContentRef, InlineLayout, RowLayoutHint};
use serde::Deserialize;
use sha2::{Digest, Sha256};

mod boundaries;
mod owners;

#[derive(Deserialize)]
struct Matrix {
    schema: String,
    contract: String,
    expected_basis: String,
    profiles: Vec<String>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    axes: Axes,
    source: String,
    source_sha256: String,
    original_text: String,
    logical_rows: usize,
    accepted_rows: Vec<RowWitness>,
    expected: String,
}

#[derive(Deserialize)]
struct Axes {
    shape: String,
    hints: String,
    fragmentation: String,
    position: String,
}

#[derive(Deserialize)]
struct RowWitness {
    source_row: usize,
    first_fragment: bool,
    text: String,
    origin: i32,
    alignment: mant_ir::TextAlignment,
}

#[test]
fn accepted_code_rows_cover_192_source_identity_configurations() {
    let matrix: Matrix = serde_json::from_str(include_str!("accepted_rows/cases.json")).unwrap();
    assert_eq!(matrix.schema, "mant-codec-accepted-row-matrix/v1");
    assert_ne!(matrix.contract, "");
    assert_ne!(matrix.expected_basis, "");
    assert_eq!(matrix.profiles.len(), 7);
    assert_eq!(matrix.cases.len(), 192);
    let mut identities = std::collections::BTreeSet::new();
    let mut axes = std::collections::BTreeSet::new();
    for case in matrix.cases {
        assert!(identities.insert(case.id.clone()));
        assert!(axes.insert((
            case.axes.shape,
            case.axes.hints,
            case.axes.fragmentation,
            case.axes.position,
        )));
        assert_eq!(
            {
                use std::fmt::Write as _;
                Sha256::digest(case.source.as_bytes()).iter().fold(
                    String::new(),
                    |mut text, byte| {
                        write!(text, "{byte:02x}").unwrap();
                        text
                    },
                )
            },
            case.source_sha256,
            "{}: original hand-IR source identity",
            case.id,
        );
        let block: Block = serde_json::from_str(&case.source).unwrap();
        let content = literal_content(&block);
        assert_eq!(
            mant_ir::inline_plain_text(content.content),
            case.original_text
        );
        assert_eq!(
            mant_ir::logical_row_count(content.content),
            case.logical_rows
        );
        content.layout.validate(content.content).unwrap();
        let expected = case
            .accepted_rows
            .iter()
            .map(|row| {
                assert!(row.source_row < case.logical_rows, "{}", case.id);
                let hint = content.layout.row_layout(row.source_row);
                assert_eq!(hint.alignment, row.alignment);
                let origin = hint.resolve(0, 0);
                assert_eq!(
                    if row.first_fragment {
                        origin.first_visual_origin
                    } else {
                        origin.continuation_origin
                    },
                    row.origin,
                    "{}: public source-row witness before export",
                    case.id,
                );
                if row.text.is_empty() {
                    String::new()
                } else {
                    format!(
                        "{}{text}",
                        " ".repeat(usize::try_from(row.origin.max(0)).unwrap()),
                        text = row.text
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            expected, case.expected,
            "{}: independent fragment map",
            case.id
        );
        assert_projection(&block, &case.expected);
    }
    assert_eq!(axes.len(), 192);
}

fn literal_content(block: &Block) -> InlineContentRef<'_> {
    match block {
        Block::Preformatted {
            children,
            inline_layout,
            ..
        } => InlineContentRef {
            content: children,
            layout: inline_layout,
        },
        Block::Table { rows, .. } => literal_content(&rows[0].cells[0].blocks[0]),
        _ => panic!("literal matrix owner"),
    }
}

fn literal(children: Vec<Inline>, layout: InlineLayout) -> Block {
    Block::Preformatted {
        children,
        inline_layout: layout,
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn table(blocks: Vec<Block>) -> Block {
    Block::Table {
        borders: mant_ir::TableBorders::default(),
        alignment: None,
        column_count: None,
        column_preferences: mant_ir::ColumnPreferences::default(),
        rows: vec![TableRow {
            column_borders: vec![],
            kind: mant_ir::TableRowKind::Data,
            cells: vec![cell(blocks)],
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn cell(blocks: Vec<Block>) -> TableCell {
    TableCell {
        preferred_width_columns: None,
        blocks,
        break_after: false,
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: None,
    }
}

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn hint(row: u32, common: i32, first: i32) -> RowLayoutHint {
    RowLayoutHint {
        row,
        indent_columns: common,
        first_line_indent_columns: first,
        alignment: mant_ir::TextAlignment::Left,
    }
}

fn assert_projection(block: &Block, expected: &str) {
    let wire = serde_json::to_string(block).unwrap();
    let restored: Block = serde_json::from_str(&wire).unwrap();
    assert_eq!(&restored, block);
    let mut query = parse_content("# Accepted rows\n", None).unwrap();
    query.document.as_mut().unwrap().blocks = vec![restored];
    assert_eq!(
        mant_ir::validate_document(query.document.as_ref().unwrap()),
        []
    );
    let original = query.clone();
    for preserve_anchors in [false, true] {
        let options = MarkdownOptions {
            preserve_anchors,
            ..MarkdownOptions::default()
        };
        let fragment = render_blocks_fragment(
            std::slice::from_ref(block),
            MarkdownFragmentOptions { preserve_anchors },
        )
        .join("\n\n");
        assert_raw_fence_payload(&fragment, expected);
        let artifact = render_addressable_markdown_with_options(&query, options);
        assert_eq!(
            artifact.text(),
            render_markdown_with_options(&query, options)
        );
        let mut current = query.clone();
        for round in 1..=2 {
            let markdown = render_markdown_with_options(&current, options);
            current = parse_content(&markdown, None).unwrap();
            assert_eq!(
                literal_payloads(current.document.as_ref().unwrap()),
                [expected],
                "round {round}: {markdown:?}"
            );
        }
        assert_eq!(
            query, original,
            "export must not rewrite source IR/link facts"
        );
    }
}

fn assert_raw_fence_payload(markdown: &str, expected: &str) {
    let mut in_fence = false;
    let mut payload = String::new();
    let mut fences = 0;
    for event in pulldown_cmark::Parser::new(markdown) {
        match event {
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::CodeBlock(_)) => {
                assert!(!in_fence);
                in_fence = true;
                fences += 1;
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::CodeBlock) => in_fence = false,
            pulldown_cmark::Event::Text(value) if in_fence => payload.push_str(&value),
            _ => {}
        }
    }
    assert_eq!(fences, 1, "{markdown:?}");
    assert_eq!(
        payload,
        format!("{expected}\n"),
        "syntax has its own one framing LF"
    );
}

fn literal_payloads(document: &Document) -> Vec<String> {
    #[derive(Default)]
    struct Payloads(Vec<String>);
    impl<'a> mant_ir::visit::Visit<'a> for Payloads {
        fn visit_block(&mut self, block: &'a Block) {
            if let Block::Preformatted { children, .. } = block {
                self.0.push(mant_ir::inline_plain_text(children));
            } else {
                mant_ir::visit::walk_block(self, block);
            }
        }
    }
    let mut payloads = Payloads::default();
    mant_ir::visit::Visit::visit_document(&mut payloads, document);
    payloads.0
}
