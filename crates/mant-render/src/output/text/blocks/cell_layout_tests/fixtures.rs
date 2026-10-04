use super::{
    Block, BlockRenderer, Inline, LayoutHint, TableCell, TableCellKind, TableRow, TableRowKind,
    TextPresentation,
};

pub(super) fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

pub(super) fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        children,
        inline_layout: mant_ir::InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }
}

pub(super) fn literal(children: Vec<Inline>) -> Block {
    Block::Preformatted {
        children,
        language: None,
        inline_layout: mant_ir::InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }
}

pub(super) fn cell(blocks: Vec<Block>) -> TableCell {
    TableCell {
        break_after: false,
        blocks,
        kind: TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: None,
    }
}

pub(super) fn table(widths: &[u16], cells: Vec<TableCell>, origin: i32) -> Block {
    Block::Table {
        rows: vec![TableRow {
            cells,
            kind: TableRowKind::Data,
        }],
        column_preferences: declared_preferences(widths),
        layout: LayoutHint {
            indent_columns: origin,
            ..LayoutHint::default()
        },
        source: None,
    }
}

pub(super) fn nested(depth: usize) -> Block {
    if depth == 0 {
        return paragraph(vec![text("LEAF")]);
    }
    table(
        &[2],
        vec![cell(vec![paragraph(vec![text("HEAD")]), nested(depth - 1)])],
        0,
    )
}

pub(super) fn renderer(decorate: &dyn Fn(TextPresentation, &str) -> String) -> BlockRenderer<'_> {
    BlockRenderer {
        names: None,
        locations: None,
        decorate,
    }
}

pub(super) fn plain(_: TextPresentation, value: &str) -> String {
    value.into()
}

pub(super) fn ansi(_: TextPresentation, value: &str) -> String {
    format!("\x1b]8;;https://example.test\x1b\\\x1b[1m{value}\x1b[0m\x1b]8;;\x1b\\")
}

pub(super) fn undecorated(value: &str) -> String {
    value
        .replace("\x1b]8;;https://example.test\x1b\\", "")
        .replace("\x1b]8;;\x1b\\", "")
        .replace("\x1b[1m", "")
        .replace("\x1b[0m", "")
}

pub(super) fn named_item() -> Block {
    use mant_ir::{
        EntryContentSlice, EntryFacts, EntryForm, EntryInlineRoot, EntryKind, EntryNameBinding,
        EntryNameEvidence, ListItem, ListKind, NameCase,
    };
    let slice = |bytes| EntryContentSlice {
        root: EntryInlineRoot::Block { index: 0 },
        path: vec![0],
        bytes,
    };
    Block::List {
        kind: ListKind::Plain,
        items: vec![ListItem {
            blocks: vec![paragraph(vec![Inline::Code {
                value: "é名-param".into(),
            }])],
            entry: Some(EntryFacts {
                id: "term-name".into(),
                kind: EntryKind::Term,
                case: NameCase::Sensitive,
                names: vec!["é名".into()],
                alias_groups: vec![],
                alias_of: None,
                value_domain: None,
                forms: vec![EntryForm {
                    parts: vec![slice(None)],
                }],
                name_bindings: vec![EntryNameBinding {
                    name: 0,
                    evidence: EntryNameEvidence::Declared,
                    occurrences: vec![EntryForm {
                        parts: vec![slice(Some(0..5))],
                    }],
                }],
            }),
            layout: mant_ir::ListItemLayout::default(),
            source: None,
        }],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }
}

pub(super) fn matched_evidence(block: Block) -> mant_protocol::ExplanationEvidence {
    use mant_protocol::{
        EvidenceClass, ExplanationContent, ExplanationContentRange, ExplanationPreview,
        OutlineNodeReference, OutlineTrail,
    };
    mant_protocol::ExplanationEvidence {
        support: None,
        support_omitted: false,
        class: EvidenceClass::ContextMention,
        ordinal: 0,
        outline: OutlineTrail {
            ancestors: Vec::new(),
            node: OutlineNodeReference::DocumentRoot {
                path: "root".into(),
                id: "root".into(),
                title: "ROOT".into(),
            },
        },
        block_path: None,
        source: None,
        bases: vec![],
        previews: vec![ExplanationPreview {
            block_path: "root/b0/r0/c0/b0/i0/b0".into(),
            source: None,
            text: "é名-param".into(),
            match_start_char: 2,
            match_end_char: 4,
            content_ranges: vec![ExplanationContentRange::BlockText {
                path: vec![
                    mant_ir::ContentBlockStep::TableCell { row: 0, column: 0 },
                    mant_ir::ContentBlockStep::Block { index: 0 },
                    mant_ir::ContentBlockStep::ListItem { index: 0 },
                    mant_ir::ContentBlockStep::Block { index: 0 },
                ],
                start_char: 2,
                end_char: 4,
            }],
            clipped_before: false,
            clipped_after: false,
        }],
        previews_omitted: false,
        entry: None,
        content: Some(ExplanationContent::Block { block }),
        details_omitted: false,
        match_details_omitted: false,
        name_bindings_omitted: false,
        content_omitted: false,
    }
}

fn declared_preferences(widths: &[u16]) -> mant_ir::ColumnPreferences {
    if widths.is_empty() {
        return mant_ir::ColumnPreferences::default();
    }
    mant_ir::ColumnPreferences {
        widths: widths.to_vec(),
        gap_columns: match widths.len() {
            n if n < 5 => 4,
            5 => 3,
            _ => 1,
        },
        advance_limit_columns: Some(256),
        extra_width_columns: Some(10),
    }
}
