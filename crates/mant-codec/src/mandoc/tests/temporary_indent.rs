//! Native temporary offset retirement and source-neutral row projection.

use std::collections::BTreeMap;

use mant_ir::{InlineLayout, LayoutHint};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use super::{Block, inline_text, parse_manual_bytes};

mod structures;

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    source_sha256: String,
    native_columns: BTreeMap<String, Option<i32>>,
}

fn parse(source: &str) -> mant_ir::Document {
    let document = parse_manual_bytes(
        std::path::Path::new("temporary-indent.7"),
        source.as_bytes(),
    )
    .expect("lower exact source");
    let json = serde_json::to_string(&document).expect("serialize actual JSON text");
    let decoded = serde_json::from_str(&json).expect("decode actual JSON text");
    assert_eq!(document, decoded);
    decoded
}

fn body(document: &mant_ir::Document) -> &[Block] {
    &document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "PROBE_START")
        .expect("probe section")
        .blocks
}

fn word_column(blocks: &[Block], word: &str) -> Option<i32> {
    blocks.iter().find_map(|block| {
        let (children, inline_layout, layout) = match block {
            Block::Paragraph {
                children,
                inline_layout,
                layout,
                ..
            }
            | Block::Preformatted {
                children,
                inline_layout,
                layout,
                ..
            } => (children, inline_layout, *layout),
            _ => return None,
        };
        root_word_column(&inline_text(children), inline_layout, layout, word)
    })
}

fn root_word_column(
    text: &str,
    inline_layout: &InlineLayout,
    layout: LayoutHint,
    word: &str,
) -> Option<i32> {
    let parent = 5 + layout.indent_columns;
    let continuation = parent + layout.continuation_indent_columns;
    text.split('\n').enumerate().find_map(|(row, text)| {
        let offset = text.find(word)?;
        let origins = inline_layout.row_layout(row).resolve(parent, continuation);
        let column = i32::try_from(mant_ir::geometry::text_width(&text[..offset])).unwrap();
        Some(origins.first_visual_origin + column)
    })
}

fn source_digest(source: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(source.as_bytes())
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

#[test]
fn temporary_indent_uses_native_numeric_prefix_and_actual_row_retirement() {
    // Every complete source in this matrix ran on the pinned pristine
    // reference before its expected columns were written. pre_ti first
    // executes pre_br, a2roffsu accepts a numeric prefix, and ascii_endline
    // consumes the independent ti register only at an actual device close.
    let matrix: serde_json::Value =
        serde_json::from_str(include_str!("temporary_indent/matrix.json")).unwrap();
    assert_eq!(matrix["oracle"]["expectations_from_product"], false);
    let cases: Vec<Case> = serde_json::from_value(matrix["cases"].clone()).unwrap();
    assert_eq!(cases.len(), 136);
    for case in cases {
        assert_eq!(source_digest(&case.source), case.source_sha256);
        let document = parse(&case.source);
        for (word, expected) in case.native_columns {
            assert_eq!(
                word_column(body(&document), &word),
                expected,
                "{} / {word}: {document:#?}",
                case.id
            );
        }
    }
}
