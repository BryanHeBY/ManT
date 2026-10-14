//! SYNOPSIS HEAD fields and BODY source origins share one execution stream.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::{Block, inline_text, parse_manual_bytes};

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    native_columns: BTreeMap<String, i32>,
    native_rows: BTreeMap<String, Vec<String>>,
}

fn roots(
    blocks: &[Block],
) -> impl Iterator<
    Item = (
        &[mant_ir::Inline],
        &mant_ir::InlineLayout,
        mant_ir::LayoutHint,
    ),
> {
    blocks.iter().filter_map(|block| match block {
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
        } => Some((children.as_slice(), inline_layout, *layout)),
        _ => None,
    })
}

#[test]
fn synopsis_head_flush_and_macro_pre_preserve_actual_temporary_origins() {
    // Every complete fixture ran pristine -Tascii/-Tutf8/-Ttree/-Tlint
    // before writing these expected columns and hard rows. pre_SY assigns
    // mt->offset; Nm preserves the current source offset. Their HEAD posts
    // flush the real field, and BODY derives its origin from term_strlen.
    // A BODY ti first closes that detached HEAD via roff_term_pre_br().
    let matrix: serde_json::Value =
        serde_json::from_str(include_str!("synopsis_rows/matrix.json")).unwrap();
    assert_eq!(matrix["oracle"]["expectations_from_product"], false);
    let cases: Vec<Case> = serde_json::from_value(matrix["cases"].clone()).unwrap();
    assert_eq!(cases.len(), 8);
    for case in cases {
        let document = parse_manual_bytes(
            std::path::Path::new("synopsis-row.7"),
            case.source.as_bytes(),
        )
        .unwrap();
        let wire = serde_json::to_string(&document).unwrap();
        let restored: mant_ir::Document = serde_json::from_str(&wire).unwrap();
        assert_eq!(document, restored);
        let blocks = &restored
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == "SYNOPSIS")
            .unwrap()
            .blocks;
        for (word, expected) in &case.native_columns {
            let column = roots(blocks).find_map(|(children, inline_layout, layout)| {
                let text = inline_text(children);
                let parent = 5 + layout.indent_columns;
                text.split('\n').enumerate().find_map(|(row, line)| {
                    let offset = line.find(word)?;
                    let continuation = parent + layout.continuation_indent_columns;
                    let first = if row == 0 { parent } else { continuation };
                    let origin = inline_layout
                        .row_layout(row)
                        .resolve(first, continuation)
                        .first_visual_origin;
                    Some(
                        origin
                            + i32::try_from(mant_ir::geometry::text_width(&line[..offset]))
                                .unwrap(),
                    )
                })
            });
            assert_eq!(
                column,
                Some(*expected),
                "{} / {word}: {restored:#?}",
                case.id
            );
        }
        let mut rows = Vec::new();
        for (children, _, layout) in roots(blocks) {
            rows.extend(std::iter::repeat_n(
                String::new(),
                usize::from(layout.spacing_before_lines),
            ));
            rows.extend(
                inline_text(children)
                    .split_terminator('\n')
                    .map(str::to_owned),
            );
        }
        let expected: Vec<_> = case.native_rows["utf8"]
            .iter()
            .map(|row| row.trim_start().to_owned())
            .collect();
        assert_eq!(
            rows, expected,
            "{}: exact physical rows: {restored:#?}",
            case.id
        );
    }
}
