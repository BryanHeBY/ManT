//! Semantic HP/IP ownership leaves real viewport geometry unchanged.
use super::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};
use serde_json::Value;

fn cases() -> Vec<Value> {
    // This published member has its own mirrored test resource. The pure
    // consumer fixture gate verifies byte equality with the engine's source.
    let matrix: Value = serde_json::from_str(include_str!("hanging_owners/cases.json")).unwrap();
    assert_eq!(matrix["header"]["count"], 24);
    matrix["cases"].as_array().unwrap().clone()
}

fn detach_owners(blocks: &mut Vec<Block>) {
    let mut retained = Vec::new();
    for mut block in std::mem::take(blocks) {
        match &mut block {
            Block::List {
                kind: ListKind::Plain,
                items,
                ..
            } if items.len() == 1 && items[0].entry.is_some() => {
                retained.append(&mut items[0].blocks);
                continue;
            }
            Block::List { items, .. } => {
                for item in items {
                    detach_owners(&mut item.blocks);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    detach_owners(&mut item.description);
                }
            }
            _ => {}
        }
        retained.push(block);
    }
    *blocks = retained;
}

fn baseline(content: &ResolvedContent) -> ResolvedContent {
    let mut original = content.clone();
    let document = original.document.as_mut().unwrap();
    detach_owners(&mut document.blocks);
    for section in &mut document.sections {
        detach_owners(&mut section.blocks);
    }
    original
}

fn cells(rendered: &RenderedDocument, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
    buffer
}

fn assert_native_cells(actual: &Buffer, detached: &Buffer, context: &Value, width: u16) {
    assert_eq!(
        actual.area, detached.area,
        "{context}: viewport width{width}"
    );
    for (index, (cell, original)) in actual.content.iter().zip(&detached.content).enumerate() {
        // A semantic Option receives the existing entry foreground theme;
        // its authored glyphs, native font modifiers and background remain.
        assert_eq!(
            (cell.symbol(), cell.modifier, cell.bg),
            (original.symbol(), original.modifier, original.bg),
            "{context}: real viewport cell {index}, width{width}"
        );
    }
}

fn first_owner(document: &Document) -> &ListItem {
    fn in_blocks(blocks: &[Block]) -> Option<&ListItem> {
        for block in blocks {
            if let Block::List {
                kind: ListKind::Plain,
                items,
                ..
            } = block
                && let Some(item) = items.iter().find(|item| item.entry.is_some())
            {
                return Some(item);
            }
        }
        None
    }
    document
        .sections
        .iter()
        .find_map(|section| in_blocks(&section.blocks))
        .unwrap()
}

fn check_native_rows(case: &Value, content: &ResolvedContent, rendered: &RenderedDocument) {
    if case["ownerNames"].as_array().unwrap().is_empty() {
        return;
    }
    let owner = first_owner(content.document.as_ref().unwrap());
    let row = rendered
        .anchor_row(owner.entry.as_ref().unwrap().id.as_str())
        .unwrap();
    let buffer = cells(rendered, 120);
    for (offset, expected) in case["nativeRows"].as_array().unwrap().iter().enumerate() {
        let expected = expected.as_str().unwrap();
        let prefixed = if expected.is_empty() {
            String::new()
        } else {
            format!("   {expected}")
        };
        let glyphs = prefixed.chars().collect::<Vec<_>>();
        for column in 0..120 {
            let scalar = glyphs.get(column).copied().unwrap_or(' ');
            assert_eq!(
                buffer[(
                    column.try_into().unwrap(),
                    (row + offset).try_into().unwrap()
                )]
                    .symbol(),
                scalar.to_string(),
                "{}: real cell {offset}:{column}",
                case["id"]
            );
        }
        let copied = rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: row + offset,
                column: 0,
            },
            focus: TextPosition {
                row: row + offset,
                column: 119,
            },
        });
        assert_eq!(copied, prefixed, "{}: whole physical row copy", case["id"]);
    }
}

#[test]
fn hanging_owners_preserve_json_real_buffer_resize_copy_and_landing_rows() {
    // Exact pristine profiles precede the fixture. Native pre_HP/post_HP and
    // pre_IP establish two independent layouts; Plain List ownership must
    // preserve them at every width. Native fixed-width rows additionally run
    // at 120 cells, with the UI's explicit three-column section origin.
    for case in cases() {
        let content =
            mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap();
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
        let bundle: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let decoded: ResolvedContent = bundle.into();
        assert_eq!(content, decoded);
        let original = baseline(&decoded);
        let view = DocumentView::new(&decoded);
        let prior = DocumentView::new(&original);
        let initial = view.render(20).text;
        for width in [20, 40, 78, 120, 20] {
            let rendered = view.render(width);
            assert_native_cells(
                &cells(&rendered, width),
                &cells(&prior.render(width), width),
                &case["id"],
                width,
            );
            if width == 120 {
                check_native_rows(&case, &decoded, &rendered);
            }
        }
        assert_eq!(view.render(20).text, initial, "{}: resize", case["id"]);
    }
}
