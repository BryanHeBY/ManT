//! Authored owners and exact source-cell observations shared by the matrices.

use super::super::*;
use mant_ir::{ColumnPreferences, InlineLayout};

const DESTINATION: &str = "https://example.test/consumer-boundary";

pub(super) fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

pub(super) fn linked(value: &str) -> Inline {
    Inline::Link {
        target: mant_ir::LinkTarget::External {
            uri: DESTINATION.into(),
        },
        title: None,
        children: vec![text(value)],
    }
}

pub(super) fn paragraph_nodes(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        children,
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }
}

pub(super) fn literal_nodes(children: Vec<Inline>) -> Block {
    Block::Preformatted {
        children,
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        language: None,
        source: None,
    }
}

pub(super) fn cell(blocks: Vec<Block>, break_after: bool) -> TableCell {
    TableCell {
        blocks,
        break_after,
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: None,
    }
}

pub(super) fn table(cells: Vec<TableCell>, preferences: ColumnPreferences, origin: i32) -> Block {
    Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells,
        }],
        column_preferences: preferences,
        layout: LayoutHint {
            indent_columns: origin,
            ..Default::default()
        },
        source: None,
    }
}

pub(super) fn json_content(blocks: Vec<Block>) -> ResolvedContent {
    let mut authored = bundle();
    let document = authored.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = blocks;
    assert!(
        mant_ir::validate_document(document).is_empty(),
        "{document:#?}"
    );
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&authored)).unwrap();
    let restored: ResolvedContent = serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
        .unwrap()
        .into();
    assert_eq!(restored, authored, "{wire}");
    restored
}

pub(super) fn copy_cells(
    rendered: &RenderedDocument,
    row: usize,
    start: usize,
    end: usize,
) -> String {
    rendered.selected_text(RenderedSelection {
        anchor: TextPosition { row, column: start },
        focus: TextPosition { row, column: end },
    })
}

pub(super) fn clipped(origin: i32) -> usize {
    usize::try_from(origin.max(0)).unwrap()
}

pub(super) fn assert_word(
    rendered: &RenderedDocument,
    value: &str,
    linked: bool,
) -> RenderedSearchMatch {
    let hits = rendered.search(value);
    assert_eq!(hits.len(), 1, "{value}: {:?}", rendered.text);
    let hit = hits[0].clone();
    assert_eq!(
        copy_cells(rendered, hit.row, hit.start_column, hit.end_column - 1),
        value
    );
    let target = linked.then(|| LinkTarget::External(ExternalUri::parse(DESTINATION).unwrap()));
    assert_eq!(
        rendered.link_target_at(hit.row, hit.start_column),
        target.as_ref()
    );
    assert_eq!(rendered.link_target_at(hit.row, hit.end_column), None);
    hit
}
