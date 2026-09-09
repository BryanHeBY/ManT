use super::*;
use crate::{
    DefinitionItem, DefinitionLayout, Inline, ListItem, ListItemLayout, ListKind, TableCell,
    TableRow, TextRange,
};

fn source() -> SourceSpan {
    SourceSpan {
        byte_range: Some(TextRange::new(
            crate::TextSize::new(7),
            crate::TextSize::new(21),
        )),
        line: 2,
        column: 3,
        end_line: Some(4),
        end_column: Some(5),
    }
}

fn layout() -> LayoutHint {
    LayoutHint {
        indent_columns: -3,
        continuation_indent_columns: 6,
        spacing_before_lines: 2,
    }
}

fn variants(source: Option<SourceSpan>) -> Vec<(Block, bool)> {
    let layout = layout();
    vec![
        (
            Block::Paragraph {
                children: Vec::new(),
                layout,
                source,
            },
            true,
        ),
        (
            Block::Preformatted {
                children: Vec::new(),
                language: Some("roff".into()),
                layout,
                source,
            },
            true,
        ),
        (
            Block::List {
                kind: ListKind::Bullet,
                compact: true,
                items: Vec::new(),
                layout,
                source,
            },
            true,
        ),
        (
            Block::DefinitionList {
                items: Vec::new(),
                declaration_groups: Vec::new(),
                compact: false,
                layout,
                source,
            },
            true,
        ),
        (
            Block::Table {
                rows: Vec::new(),
                layout,
                source,
            },
            true,
        ),
        (
            Block::Equation {
                value: "x + y".into(),
                display: true,
                layout,
                source,
            },
            true,
        ),
        (
            Block::Unsupported {
                name: Some("unknown".into()),
                text: "BODY".into(),
                layout,
                source,
            },
            true,
        ),
        (Block::VerticalSpace { lines: 3, source }, false),
        (Block::ThematicBreak { source }, false),
    ]
}

#[test]
fn every_block_layout_accessor_agrees_and_keeps_original_source() {
    for source in [None, Some(source())] {
        for (mut block, carries_layout) in variants(source) {
            assert_eq!(block_source(&block), source);
            assert_eq!(block_layout(&block), carries_layout.then_some(&layout()));
            let original = block.clone();
            if let Some(hint) = block_layout_mut(&mut block) {
                hint.indent_columns = 12;
            }
            assert_eq!(block_source(&block), source);
            if carries_layout {
                let hint = block_layout(&block).unwrap();
                assert_eq!(hint.indent_columns, 12);
                assert_eq!(hint.continuation_indent_columns, 6);
                assert_eq!(hint.spacing_before_lines, 2);
            } else {
                assert_eq!(block, original);
            }
        }
    }
}

#[test]
fn reparenting_all_variants_changes_only_root_origins() {
    for (old_parent, new_parent) in [(10, -5), (-5, 10), (7, 7)] {
        for (block, carries_layout) in variants(Some(source())) {
            let mut blocks = vec![block.clone()];
            rebase_roots(&mut blocks, old_parent, new_parent);
            let mut expected = block;
            if carries_layout {
                block_layout_mut(&mut expected).unwrap().indent_columns =
                    -3 + old_parent - new_parent;
                assert_eq!(
                    block_layout(&blocks[0]).unwrap().indent_columns + new_parent,
                    -3 + old_parent
                );
            }
            assert_eq!(blocks, [expected]);
        }
    }
}

#[test]
fn nested_reparenting_never_translates_descendants_or_source_twice() {
    let paragraph = Block::Paragraph {
        children: vec![
            Inline::anchor("target"),
            Inline::Text {
                value: "UNCHANGED".into(),
            },
        ],
        layout: layout(),
        source: Some(source()),
    };
    let table = Block::Table {
        rows: vec![TableRow {
            cells: vec![TableCell {
                blocks: vec![paragraph],
                column_span: 1,
                row_span: 1,
                alignment: None,
            }],
        }],
        layout: layout(),
        source: Some(source()),
    };
    let definition = Block::DefinitionList {
        items: vec![DefinitionItem {
            terms: vec![vec![Inline::Text {
                value: "TERM".into(),
            }]],
            description: vec![table],
            entry: None,
            source: Some(source()),
            layout: DefinitionLayout::default(),
        }],
        declaration_groups: Vec::new(),
        compact: false,
        layout: layout(),
        source: Some(source()),
    };
    let mut blocks = vec![Block::List {
        kind: ListKind::Ordered { start: Some(5) },
        compact: false,
        items: vec![ListItem {
            blocks: vec![definition],
            entry: None,
            source: Some(source()),
            layout: ListItemLayout::default(),
        }],
        layout: layout(),
        source: Some(source()),
    }];
    let original = blocks.clone();
    rebase_roots(&mut blocks, 10, 4);
    assert_eq!(block_layout(&blocks[0]).unwrap().indent_columns, 3);
    block_layout_mut(&mut blocks[0]).unwrap().indent_columns = -3;
    assert_eq!(blocks, original);
}

#[test]
fn extreme_parent_origins_saturate_only_the_moved_root() {
    for (old_parent, new_parent, expected) in [
        (i32::MAX, i32::MIN, i32::MAX),
        (i32::MIN, i32::MAX, i32::MIN),
        (i32::MAX, i32::MAX, -3),
        (i32::MIN, i32::MIN, -3),
    ] {
        let mut blocks = vec![variants(Some(source())).remove(0).0];
        rebase_roots(&mut blocks, old_parent, new_parent);
        assert_eq!(block_layout(&blocks[0]).unwrap().indent_columns, expected);
        assert_eq!(block_source(&blocks[0]), Some(source()));
    }
}
