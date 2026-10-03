//! Inspect actual terminal cells, including selected and folded narrow rows.

use mant_ir::EntryKind;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier},
    widgets::Widget,
};

use super::{HashSet, node};
use crate::{NavKind, theme};

#[test]
fn semantic_outline_roles_remain_distinct_in_selected_and_narrow_rows() {
    let cases = [
        (NavKind::Section, "Section", theme::BLUE),
        (NavKind::EntryGroup, "Entries · 4", theme::MAROON),
        (NavKind::ReferenceGroup, "Doc Refs · 12", theme::LAVENDER),
        (
            NavKind::Reference,
            "↗ printf(3) · Unicode reference with a long label 文档",
            theme::LINK,
        ),
        (
            NavKind::ReferenceNotice,
            "! Doc Refs limited",
            theme::YELLOW,
        ),
        (NavKind::Entry(EntryKind::Value), "Value", theme::BLUE),
        (NavKind::Tldr, "TLDR", theme::MAUVE),
    ];
    for (kind, title, foreground) in cases {
        let mut root = node("Document");
        root.id = "document".into();
        root.has_children = true;
        let mut parent = node("Parent");
        parent.id = "parent".into();
        parent.parent_id = Some(root.id.clone());
        parent.depth = 1;
        parent.has_children = true;
        let mut child = node(title);
        child.parent_id = Some(parent.id.clone());
        child.depth = 2;
        child.kind = kind;
        if kind == NavKind::ReferenceNotice {
            child.full_title = Some("! Doc Refs inventory limited by navigation budget".into());
        }
        child.has_children = matches!(kind, NavKind::EntryGroup | NavKind::ReferenceGroup);
        let nodes = [root, parent, child];
        for width in [24_u16, 32, 48] {
            for selected in [false, true] {
                for expanded in [false, true] {
                    let rows = super::super::rows(
                        &nodes,
                        &[2],
                        if selected { 2 } else { usize::MAX },
                        &if expanded {
                            HashSet::from(["node".into()])
                        } else {
                            HashSet::new()
                        },
                        false,
                        usize::from(width),
                    );
                    assert_eq!(super::super::node_row_range(&rows, 2), Some(0..rows.len()));
                    assert_row_styles(&rows, width, kind, foreground, selected);
                }
            }
        }
    }
}

fn assert_row_styles(
    rows: &[super::super::NavigationRow],
    width: u16,
    kind: NavKind,
    foreground: Color,
    selected: bool,
) {
    let height = u16::try_from(rows.len()).unwrap();
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
    for (row_index, row) in rows.iter().enumerate() {
        assert!(row.line.width() <= usize::from(width));
        let y = u16::try_from(row_index).unwrap();
        row.line
            .clone()
            .render(Rect::new(0, y, width, 1), &mut buffer);
        let x = u16::try_from(row.line.spans[0].width()).unwrap();
        let cell = &buffer[(x, y)];
        let expected = if selected && kind != NavKind::Tldr {
            theme::SELECTED_TEXT
        } else {
            foreground
        };
        assert_eq!(
            cell.fg, expected,
            "{kind:?}, width={width}, selected={selected}"
        );
        if selected {
            let background = if kind == NavKind::Tldr {
                theme::TLDR_SELECTED
            } else {
                theme::SELECTED
            };
            assert_eq!(cell.bg, background);
            assert!(cell.modifier.contains(Modifier::BOLD));
        }
        if kind == NavKind::Reference {
            assert!(cell.modifier.contains(Modifier::UNDERLINED));
        }
        if matches!(kind, NavKind::EntryGroup | NavKind::ReferenceGroup) {
            assert!(cell.modifier.contains(Modifier::BOLD));
        }
    }
}
