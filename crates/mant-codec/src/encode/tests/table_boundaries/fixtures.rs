//! Shared table cells, nested containers, and exact readback assertions.
use super::super::{
    Block, DefinitionItem, EntryFacts, EntryKind, Inline, LayoutHint, ListItem, ListKind,
    MarkdownFragmentOptions, MarkdownOptions, NameCase, TableCell, TableRow, paragraph,
    parse_content, render_blocks_fragment,
};

pub(super) fn cell(value: &str, break_after: bool, completed: u16) -> TableCell {
    let mut blocks = if value.is_empty() {
        Vec::new()
    } else {
        vec![paragraph(vec![Inline::Text {
            value: value.into(),
        }])]
    };
    if completed > 0 {
        blocks.push(Block::VerticalSpace {
            lines: completed,
            source: None,
        });
    }
    TableCell {
        break_after,
        kind: mant_ir::TableCellKind::Text,
        blocks,
        column_span: 1,
        row_span: 1,
        alignment: None,
    }
}

pub(super) fn table(cells: Vec<TableCell>) -> Block {
    Block::Table {
        column_preferences: mant_ir::ColumnPreferences::default(),
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells,
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

pub(super) fn cell_blocks(blocks: Vec<Block>, break_after: bool) -> TableCell {
    TableCell {
        blocks,
        ..cell("", break_after, 0)
    }
}

pub(super) fn gap(rows: u16) -> Block {
    Block::VerticalSpace {
        lines: rows,
        source: None,
    }
}

pub(super) fn owned(value: &str, id: &str) -> Block {
    Block::List {
        kind: ListKind::Plain,
        items: vec![ListItem {
            blocks: vec![paragraph(vec![Inline::Text {
                value: value.into(),
            }])],
            entry: Some(EntryFacts {
                id: id.into(),
                kind: EntryKind::Term,
                case: NameCase::Sensitive,
                names: Vec::new(),
                forms: Vec::new(),
                name_bindings: Vec::new(),
                alias_groups: Vec::new(),
                alias_of: None,
                value_domain: None,
            }),
            layout: mant_ir::ListItemLayout::default(),
            source: None,
        }],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }
}

pub(super) fn literal(value: &str) -> Block {
    Block::Preformatted {
        children: vec![Inline::Text {
            value: value.into(),
        }],
        inline_layout: mant_ir::InlineLayout::default(),
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }
}

pub(super) fn wrap(block: Block, kind: &str) -> Block {
    match kind {
        "table" => table(vec![cell_blocks(vec![block], false)]),
        "list" => Block::List {
            kind: ListKind::Plain,
            items: vec![ListItem {
                blocks: vec![block],
                layout: mant_ir::ListItemLayout::default(),
                entry: None,
                source: None,
            }],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        },
        "definition" => Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![DefinitionItem {
                terms: vec![
                    vec![Inline::Text {
                        value: "TERM".into(),
                    }]
                    .into(),
                ],
                description: vec![block],
                head_body_relation: mant_ir::HeadBodyRelation::Separate,
                layout: mant_ir::DefinitionLayout::default(),
                entry: None,
                source: None,
            }],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        },
        _ => unreachable!(),
    }
}

pub(super) fn literal_readback(block: &Block) -> String {
    let markdown = render_blocks_fragment(
        std::slice::from_ref(block),
        MarkdownFragmentOptions::default(),
    )
    .join("\n\n");
    let parsed = parse_content(&markdown, None).unwrap();
    let [Block::Preformatted { children, .. }] =
        parsed.document.as_ref().unwrap().blocks.as_slice()
    else {
        panic!("one literal table readback: {markdown}");
    };
    mant_ir::inline_plain_text(children)
}

pub(super) fn assert_readback(block: &Block, expected: &str) {
    assert_eq!(literal_readback(block), expected);
    let wire = serde_json::to_string(block).unwrap();
    let restored: Block = serde_json::from_str(&wire).unwrap();
    assert_eq!(&restored, block);
    assert_eq!(literal_readback(&restored), expected, "{wire}");
}

pub(super) fn assert_mapped_readback(block: &Block, expected: &str, owners: &[(&str, &str)]) {
    let wire = serde_json::to_string(block).unwrap();
    let restored: Block = serde_json::from_str(&wire).unwrap();
    assert_eq!(&restored, block);
    for block in [block, &restored] {
        assert_eq!(literal_readback(block), expected);
        let blocks = std::slice::from_ref(block);
        let original = mant_ir::content_entries(blocks);
        let rendered =
            super::blocks::render_blocks_with_entries(blocks, MarkdownOptions::default(), true);
        assert_eq!(rendered.entries.len(), owners.len());
        for (id, text) in owners {
            let mapped = rendered
                .entries
                .iter()
                .find(|entry| entry.owner.facts().unwrap().id.as_str() == *id)
                .unwrap();
            let source = original
                .iter()
                .find(|entry| entry.owner().facts().unwrap().id.as_str() == *id)
                .unwrap();
            assert!(std::ptr::eq(
                mapped.owner.facts().unwrap(),
                source.owner().facts().unwrap()
            ));
            // Fixed fence prefix is four bytes. Independent expected payload
            // positions use UTF-8 bytes, never scalar/display-cell counts.
            let start = 4 + expected.find(*text).unwrap();
            assert_eq!(mapped.start..mapped.end, start..start + text.len());
            assert_eq!(&rendered.text[mapped.start..mapped.end], *text);
        }
    }
}
