//! Check semantic group colors after the complete application paints a frame.

use super::*;

#[test]
fn entries_and_references_keep_distinct_group_colors_in_the_final_frame() {
    let mut content = navigation_bundle();
    content.document.as_mut().unwrap().sections[0]
        .blocks
        .push(AstBlock::Paragraph {
            children: vec![Inline::Link {
                target: mant_ir::LinkTarget::Manual {
                    name: "printf".into(),
                    manual_section: Some("3".into()),
                },
                title: None,
                children: vec![Inline::Text {
                    value: "printf(3)".into(),
                }],
            }],
            layout: LayoutHint::default(),
            source: None,
        });
    for width in [80, 120] {
        for expanded in [false, true] {
            let mut app = App::new(&content);
            if expanded {
                app.expanded.extend(
                    app.session
                        .document
                        .navigation()
                        .iter()
                        .map(|node| node.id.clone()),
                );
            }
            let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            for (kind, label, color) in [
                (NavKind::EntryGroup, "Entries", theme::MAROON),
                (NavKind::ReferenceGroup, "Doc Refs", theme::LAVENDER),
            ] {
                assert_group_cell(&app, terminal.backend().buffer(), kind, label, color);
            }
        }
    }
}

fn assert_group_cell(
    app: &App,
    buffer: &ratatui::buffer::Buffer,
    kind: NavKind,
    label: &str,
    color: ratatui::style::Color,
) {
    let index = app
        .session
        .document
        .navigation()
        .iter()
        .position(|node| node.kind == kind)
        .unwrap();
    let row = app
        .geometry
        .navigation_rows
        .iter()
        .position(|node| *node == index)
        .unwrap();
    let area = app.geometry.navigation;
    let y = area.y + u16::try_from(row).unwrap();
    let x = (area.x..area.right())
        .find(|x| {
            (*x..area.right())
                .map(|column| buffer[(column, y)].symbol())
                .collect::<String>()
                .starts_with(label)
        })
        .expect("group label is painted in the outline");
    for column in x..x + u16::try_from(label.len()).unwrap() {
        let cell = &buffer[(column, y)];
        assert_eq!(cell.fg, color, "{kind:?}");
        assert_eq!(cell.bg, theme::SIDEBAR);
        assert!(cell.modifier.contains(ratatui::style::Modifier::BOLD));
    }
}
