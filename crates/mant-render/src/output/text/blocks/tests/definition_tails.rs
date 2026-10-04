//! Apply a shared row's geometry after all of its accepted content arrives.
use super::*;
use mant_ir::{DefinitionTerm, HeadBodyRelation, InlineLayout, RowLayoutHint};

#[test]
fn open_head_rows_apply_hints_after_body_composition_for_plain_and_ansi() {
    for head in ["X", "X\n", "X\n\n", "\n", ""] {
        for parent in [-3_i32, 0, 3] {
            for correction in [-4_i32, 0, 4] {
                for relation in [HeadBodyRelation::joined(), HeadBodyRelation::separated()] {
                    let last_row = head.bytes().filter(|&byte| byte == b'\n').count();
                    let content = if head.is_empty() {
                        vec![Inline::anchor("head")]
                    } else {
                        vec![Inline::Strong {
                            children: vec![Inline::Text { value: head.into() }],
                        }]
                    };
                    let item = DefinitionItem {
                        terms: vec![DefinitionTerm {
                            content,
                            inline_layout: InlineLayout {
                                row_hints: vec![RowLayoutHint {
                                    row: u32::try_from(last_row).unwrap(),
                                    indent_columns: correction,
                                }],
                            },
                        }],
                        description: vec![paragraph("Y", 0)],
                        head_body_relation: relation,
                        layout: mant_ir::DefinitionLayout {
                            body_alignment: mant_ir::DefinitionBodyAlignment::AfterTerm,
                            ..Default::default()
                        },
                        entry: None,
                        source: None,
                    };
                    let mut expected = String::new();
                    if head.is_empty() {
                        expected
                            .push_str(&" ".repeat(usize::try_from((parent + 4).max(0)).unwrap()));
                        expected.push('Y');
                    } else {
                        let rows = head.split('\n').collect::<Vec<_>>();
                        for (index, row) in rows.iter().enumerate() {
                            if index > 0 {
                                expected.push('\n');
                            }
                            let last = index == last_row;
                            if !row.is_empty() || last {
                                let indent = parent + if last { correction } else { 0 };
                                expected
                                    .push_str(&" ".repeat(usize::try_from(indent.max(0)).unwrap()));
                            }
                            expected.push_str(row);
                            if last {
                                if !relation.joins_without_separator() {
                                    expected.push(' ');
                                }
                                expected.push('Y');
                            }
                        }
                    }
                    let block = Block::DefinitionList {
                        items: vec![item],
                        compact: true,
                        declaration_groups: vec![],
                        layout: LayoutHint::default(),
                        source: None,
                    };
                    let wire = serde_json::to_string(&block).unwrap();
                    let restored: Block = serde_json::from_str(&wire).unwrap();
                    for decorated in [false, true] {
                        let paint = |_: TextPresentation, value: &str| {
                            if decorated {
                                format!("\x1b[1m{value}\x1b[0m")
                            } else {
                                value.into()
                            }
                        };
                        let renderer = BlockRenderer {
                            names: None,
                            locations: None,
                            decorate: &paint,
                        };
                        let text = renderer
                            .render_blocks(std::slice::from_ref(&restored), parent)
                            .replace("\x1b[1m", "")
                            .replace("\x1b[0m", "");
                        assert_eq!(
                            text, expected,
                            "head={head:?}/parent={parent}/hint={correction}/relation={relation:?}"
                        );
                    }
                }
            }
        }
    }
}
