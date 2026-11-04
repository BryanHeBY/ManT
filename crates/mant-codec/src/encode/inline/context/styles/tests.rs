use super::*;
use crate::encode::inline::context::serialization::{Framing, render_inline_pieces};
use crate::encode::inline::context::work::{PROJECTED_SCALARS, WHOLE_PARSES};

#[test]
fn actual_parser_work_is_bounded_by_piece_bytes_not_output_history() {
    let mut previous = None;
    for count in [64, 256, 1024] {
        let mut pieces = (0..count)
            .map(|index| {
                InlinePiece::styled(
                    if index % 2 == 0 {
                        "--arg=".into()
                    } else {
                        "reuse_level".into()
                    },
                    if index % 2 == 0 { "**" } else { "*" },
                    if index % 2 == 0 { "__" } else { "_" },
                )
            })
            .collect::<Vec<_>>();
        let bytes: usize = pieces.iter().map(|piece| piece.rendered.len()).sum();
        PARSED_BYTES.with(|work| work.set(0));
        WHOLE_PARSES.with(|work| work.set(0));
        PROJECTED_SCALARS.with(|work| work.set(0));
        let output = render_inline_pieces(&mut pieces, Framing::Raw).unwrap();
        let work = PARSED_BYTES.with(std::cell::Cell::get);
        // Two cached child checks add at most four piece parsings to
        // the existing bounded flank witnesses. No historical prefix
        // is rescanned as the sequence grows.
        assert!(work <= 24 * (bytes + 12 * count) + 4 * output.len());
        assert_eq!(WHOLE_PARSES.with(std::cell::Cell::get), 1);
        assert!(PROJECTED_SCALARS.with(std::cell::Cell::get) <= 3 * bytes);
        if let Some(previous) = previous {
            assert!(work <= previous * 5);
        }
        previous = Some(work);
    }
}

#[test]
fn nested_parser_work_is_bounded_for_separate_length_and_depth_axes() {
    // Recursive formatting revisits an accepted child at every outer
    // style. Keep depth separate from flat sequence length: this budget
    // is O(depth * (text bytes + depth)), not an arbitrary-depth linearity
    // claim. Both counters observe actual parser/projection loop work.
    let axes = [
        (4, 64),
        (4, 256),
        (4, 1024),
        (1, 128),
        (4, 128),
        (16, 128),
        (64, 128),
    ];
    for (depth, bytes) in axes {
        let mut node = mant_ir::Inline::Text {
            value: "x".repeat(bytes),
        };
        for level in 0..depth {
            node = if level % 2 == 0 {
                mant_ir::Inline::Strong {
                    children: vec![node],
                }
            } else {
                mant_ir::Inline::Emphasis {
                    children: vec![node],
                }
            };
        }
        PARSED_BYTES.with(|work| work.set(0));
        PROJECTED_SCALARS.with(|work| work.set(0));
        let _ = super::super::render_inline_raw(
            &[node],
            crate::encode::MarkdownOptions::default(),
            false,
        );
        assert!(PARSED_BYTES.with(std::cell::Cell::get) <= 8 * depth * (bytes + 8 * depth + 8));
        assert!(PROJECTED_SCALARS.with(std::cell::Cell::get) <= 3 * depth * bytes);
    }
}
