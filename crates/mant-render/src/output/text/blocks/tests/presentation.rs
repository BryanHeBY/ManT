//! Synthetic owner layout, including structural marker and shared-row seams.
use super::*;
use mant_ir::{DefinitionLayout, DefinitionTerm, HeadBodyRelation, InlineLayout, RowLayoutHint};

fn paragraph_with_hint(value: &str, hint: RowLayoutHint) -> Block {
    Block::Paragraph {
        children: vec![Inline::Text {
            value: value.into(),
        }],
        inline_layout: InlineLayout {
            row_hints: vec![hint],
        },
        layout: LayoutHint::default(),
        source: None,
    }
}

fn alignment_origin(
    alignment: mant_ir::TextAlignment,
    width: usize,
    origin: usize,
    text: usize,
) -> usize {
    let free = width - origin - text;
    origin
        + match alignment {
            mant_ir::TextAlignment::Left => 0,
            mant_ir::TextAlignment::Center => free / 2,
            mant_ir::TextAlignment::Right => free,
        }
}

#[test]
fn definition_rows_align_after_head_and_body_are_composed() {
    for width in [20, 40] {
        for alignment in [
            mant_ir::TextAlignment::Left,
            mant_ir::TextAlignment::Center,
            mant_ir::TextAlignment::Right,
        ] {
            for relation in [
                HeadBodyRelation::Separate,
                HeadBodyRelation::joined(),
                HeadBodyRelation::separated(),
            ] {
                for aligned_head in [false, true] {
                    let head_hint = RowLayoutHint {
                        first_line_indent_columns: 2,
                        alignment: if aligned_head {
                            alignment
                        } else {
                            mant_ir::TextAlignment::Left
                        },
                        ..Default::default()
                    };
                    let block = Block::DefinitionList {
                        compact: true,
                        declaration_groups: vec![],
                        items: vec![DefinitionItem {
                            terms: vec![DefinitionTerm {
                                content: vec![Inline::Strong {
                                    children: vec![Inline::Text {
                                        value: "HEAD".into(),
                                    }],
                                }],
                                inline_layout: InlineLayout {
                                    row_hints: vec![head_hint],
                                },
                            }],
                            description: vec![paragraph_with_hint(
                                "BODY",
                                RowLayoutHint {
                                    alignment,
                                    ..Default::default()
                                },
                            )],
                            head_body_relation: relation,
                            layout: DefinitionLayout {
                                body_alignment: mant_ir::DefinitionBodyAlignment::AfterTerm,
                                ..Default::default()
                            },
                            entry: None,
                            source: None,
                        }],
                        layout: LayoutHint::default(),
                        source: None,
                    };
                    let expected = match relation {
                        HeadBodyRelation::Separate => format!(
                            "{}HEAD\n{}BODY",
                            " ".repeat(alignment_origin(head_hint.alignment, width, 2, 4)),
                            " ".repeat(alignment_origin(alignment, width, 4, 4))
                        ),
                        HeadBodyRelation::Shared { word_boundary } => {
                            let joined = word_boundary == mant_ir::DefinitionWordBoundary::Joined;
                            let words = if joined { "HEADBODY" } else { "HEAD BODY" };
                            format!(
                                "{}{words}",
                                " ".repeat(alignment_origin(alignment, width, 2, words.len()))
                            )
                        }
                    };
                    for paint in [false, true] {
                        let decorate = |_: TextPresentation, value: &str| {
                            if paint {
                                format!("\x1b[1m{value}\x1b[0m")
                            } else {
                                value.into()
                            }
                        };
                        let renderer = BlockRenderer {
                            width: Some(width),
                            names: None,
                            locations: None,
                            decorate: &decorate,
                        };
                        assert_eq!(
                            renderer
                                .render_blocks(std::slice::from_ref(&block), 0)
                                .replace("\x1b[1m", "")
                                .replace("\x1b[0m", ""),
                            expected,
                            "{width} {alignment:?} {relation:?} {aligned_head}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn marker_stays_fixed_while_first_body_origin_and_alignment_move() {
    for width in [20, 40] {
        for alignment in [
            mant_ir::TextAlignment::Left,
            mant_ir::TextAlignment::Center,
            mant_ir::TextAlignment::Right,
        ] {
            for delta in [0, 2, 4] {
                for (kind, marker) in [
                    (ListKind::Bullet, "• "),
                    (ListKind::Dash, "- "),
                    (ListKind::Ordered { start: Some(1) }, "1. "),
                ] {
                    let block = Block::List {
                        kind,
                        compact: true,
                        items: vec![ListItem {
                            blocks: vec![paragraph_with_hint(
                                "BODY",
                                RowLayoutHint {
                                    first_line_indent_columns: delta,
                                    alignment,
                                    ..Default::default()
                                },
                            )],
                            entry: None,
                            source: None,
                            layout: mant_ir::ListItemLayout::default(),
                        }],
                        layout: LayoutHint::default(),
                        source: None,
                    };
                    let marker_width = marker.chars().count();
                    let body_origin = alignment_origin(
                        alignment,
                        width,
                        marker_width + usize::try_from(delta).unwrap(),
                        4,
                    );
                    let renderer = BlockRenderer {
                        width: Some(width),
                        ..super::super::super::plain_renderer()
                    };
                    assert_eq!(
                        renderer.render_blocks(&[block], 0),
                        format!("{marker}{}BODY", " ".repeat(body_origin - marker_width)),
                        "{width} {alignment:?} {delta} {kind:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn table_intrinsic_measurement_never_uses_page_alignment_padding() {
    for width in [20, 40] {
        for alignment in [
            mant_ir::TextAlignment::Center,
            mant_ir::TextAlignment::Right,
        ] {
            for declared in [false, true] {
                let table = Block::Table {
                    column_preferences: mant_ir::ColumnPreferences {
                        widths: if declared { vec![6, 6] } else { vec![] },
                        ..Default::default()
                    },
                    rows: vec![mant_ir::TableRow {
                        kind: mant_ir::TableRowKind::Data,
                        cells: ["X", "Y"]
                            .map(|value| TableCell {
                                blocks: vec![paragraph_with_hint(
                                    value,
                                    RowLayoutHint {
                                        alignment,
                                        ..Default::default()
                                    },
                                )],
                                break_after: false,
                                kind: mant_ir::TableCellKind::Text,
                                column_span: 1,
                                row_span: 1,
                                alignment: None,
                            })
                            .into(),
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                };
                let renderer = BlockRenderer {
                    width: Some(width),
                    ..super::super::super::plain_renderer()
                };
                let (text, visits) =
                    super::super::visits::observe(|| renderer.render_blocks(&[table], 0));
                let (first, second) = if declared {
                    (
                        alignment_origin(alignment, 6, 0, 1),
                        alignment_origin(alignment, width, 8, 1),
                    )
                } else {
                    (0, alignment_origin(alignment, width, 4, 1))
                };
                let expected = if declared {
                    format!("{}X{}Y", " ".repeat(first), " ".repeat(second - first - 1))
                } else {
                    format!("X | {}Y", " ".repeat(second - 4))
                };
                assert_eq!(text, expected);
                assert_eq!(visits.blocks, 3, "one table and one visit per paragraph");
                assert_eq!(visits.inline_fragments, 2);
            }
        }
    }
}
