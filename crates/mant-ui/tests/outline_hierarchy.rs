//! Source-backed hierarchy and actual terminal geometry must agree.

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use mant_ir::ResolvedContent;
use mant_protocol::OutlineNode;
use mant_ui::{App, DocumentView, NavKind, NavNode};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

const GIT_FIXTURES: [&str; 2] = ["archlinux/git.1.gz", "fedora44/git.1.zst"];
const ROOT_TITLES: [&str; 7] = [
    "SYNOPSIS",
    "DESCRIPTION",
    "OPTIONS",
    "REPORTING BUGS",
    "SEE ALSO",
    "GIT",
    "NOTES",
];
const ENVIRONMENT_CHILDREN: [&str; 5] = [
    "System",
    "The Git Repository",
    "Git Commits",
    "Git Diffs",
    "other",
];

fn git_fixture(relative: &str) -> ResolvedContent {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real")
        .join(relative);
    let document = mant_loader::parse_manual_source(&path).expect("checked-in Git fixture");
    ResolvedContent {
        address: None,
        label: relative.into(),
        document: Some(document),
        tldr: None,
    }
}

fn section_node<'a>(view: &'a DocumentView, title: &str) -> &'a NavNode {
    view.navigation()
        .iter()
        .find(|node| node.kind == NavKind::Section && node.title == title)
        .expect("source-authored section remains in navigation")
}

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn draw(app: &mut App, terminal: &mut Terminal<TestBackend>) {
    terminal.draw(|frame| app.draw(frame)).unwrap();
}

fn terminal(app: &mut App) -> Terminal<TestBackend> {
    // Keep both the original and expanded reference inventory visible. The
    // assertion concerns columns, not scroll offsets or version-specific counts.
    let mut terminal = Terminal::new(TestBackend::new(188, 512)).unwrap();
    draw(app, &mut terminal);
    for _ in 0..16 {
        key(app, KeyCode::Char('>'));
    }
    draw(app, &mut terminal);
    terminal
}

fn label_position(buffer: &Buffer, title: &str) -> Option<(u16, u16)> {
    // Inspect real cells in the sidebar, excluding both document text and its
    // duplicate headings. All labels used here are ASCII; Unicode glyph layout
    // has its own actual-buffer regressions.
    let title_width = u16::try_from(title.len()).unwrap();
    for y in 5..buffer.area.height.saturating_sub(1) {
        for x in 0..64_u16.saturating_sub(title_width) {
            if title.bytes().enumerate().all(|(offset, byte)| {
                buffer[(x + u16::try_from(offset).unwrap(), y)].symbol()
                    == char::from(byte).to_string()
            }) {
                let before = (0..x)
                    .map(|column| buffer[(column, y)].symbol())
                    .collect::<String>();
                let after = (x + title_width..64)
                    .map(|column| buffer[(column, y)].symbol())
                    .collect::<String>();
                // In particular, GIT must not accidentally match GIT COMMANDS
                // or an entry/reference with a matching substring.
                if !before.chars().any(char::is_alphanumeric)
                    && !after.trim_start().starts_with(char::is_alphanumeric)
                {
                    return Some((x, y));
                }
            }
        }
    }
    None
}

fn position(terminal: &Terminal<TestBackend>, title: &str) -> (u16, u16) {
    label_position(terminal.backend().buffer(), title)
        .unwrap_or_else(|| panic!("sidebar label {title:?} is not visible"))
}

fn select_label(app: &mut App, terminal: &Terminal<TestBackend>, title: &str) {
    let (column, row) = position(terminal, title);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

fn assert_root_columns(terminal: &Terminal<TestBackend>, expected: Option<u16>) -> u16 {
    let column = position(terminal, ROOT_TITLES[0]).0;
    if let Some(expected) = expected {
        assert_eq!(column, expected, "expansion moved root labels");
    }
    for title in ROOT_TITLES {
        assert_eq!(position(terminal, title).0, column, "root {title}");
    }
    column
}

#[test]
fn both_git_sources_preserve_root_sections_and_real_subsections_in_ir_and_outlines() {
    for fixture in GIT_FIXTURES {
        let content = git_fixture(fixture);
        let document = content.document.as_ref().unwrap();
        let outline = mant_query::build_outline(&content).unwrap();
        let view = DocumentView::new(&content);
        for title in ROOT_TITLES {
            let section = document
                .sections
                .iter()
                .find(|section| section.heading.plain_text() == title)
                .unwrap_or_else(|| panic!("{fixture}: {title} must be a source .SH"));
            assert!(outline.nodes.iter().any(|node| matches!(node,
                OutlineNode::DocumentSection { id, title: found, .. } if id == &section.id && found == title)));
            let node = section_node(&view, title);
            assert_eq!(node.depth, 0, "{fixture}: {title}");
            assert_eq!(node.parent_id, None, "{fixture}: {title}");
        }
        let environment = document
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == "ENVIRONMENT VARIABLES")
            .unwrap();
        assert_eq!(
            environment
                .children
                .iter()
                .map(|section| section.heading.plain_text())
                .collect::<Vec<_>>(),
            ENVIRONMENT_CHILDREN
        );
        let parent = section_node(&view, "ENVIRONMENT VARIABLES");
        for title in ENVIRONMENT_CHILDREN {
            let child = section_node(&view, title);
            assert_eq!(child.depth, 1, "{fixture}: {title}");
            assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
        }
        for (title, kind) in [
            ("DESCRIPTION", NavKind::ReferenceGroup),
            ("OPTIONS", NavKind::EntryGroup),
            ("SEE ALSO", NavKind::ReferenceGroup),
            ("GIT", NavKind::ReferenceGroup),
        ] {
            let parent = section_node(&view, title);
            assert!(
                view.navigation().iter().any(|node| node.kind == kind
                    && node.parent_id.as_deref() == Some(parent.id.as_str())
                    && node.depth == 1),
                "{fixture}: {kind:?} remains owned by {title}"
            );
        }
    }
}

#[test]
fn real_git_sibling_columns_survive_reference_group_expansion_and_subsection_folding() {
    for fixture in GIT_FIXTURES {
        let content = git_fixture(fixture);
        let mut app = App::new(&content);
        let mut terminal = terminal(&mut app);
        let root_column = assert_root_columns(&terminal, None);
        for (title, next_root) in [("SEE ALSO", "GIT"), ("GIT", "NOTES")] {
            let before_row = position(&terminal, next_root).1;
            select_label(&mut app, &terminal, title);
            key(&mut app, KeyCode::Right); // Select its reference group.
            key(&mut app, KeyCode::Char(' '));
            draw(&mut app, &mut terminal);
            assert_root_columns(&terminal, Some(root_column));
            assert!(
                position(&terminal, next_root).1 > before_row,
                "{fixture}: {title} reference expansion must actually expose children"
            );
            key(&mut app, KeyCode::Char(' '));
            draw(&mut app, &mut terminal);
            assert_root_columns(&terminal, Some(root_column));
            assert_eq!(position(&terminal, next_root).1, before_row);
        }
        select_label(&mut app, &terminal, "ENVIRONMENT VARIABLES");
        key(&mut app, KeyCode::Left);
        draw(&mut app, &mut terminal);
        for title in ENVIRONMENT_CHILDREN {
            assert_eq!(
                label_position(terminal.backend().buffer(), title),
                None,
                "{fixture}: collapsed .SS {title}"
            );
        }
        assert_root_columns(&terminal, Some(root_column));
        key(&mut app, KeyCode::Right);
        draw(&mut app, &mut terminal);
        for title in ENVIRONMENT_CHILDREN {
            assert!(
                position(&terminal, title).0 > root_column,
                "{fixture}: nested {title}"
            );
        }
        assert_root_columns(&terminal, Some(root_column));
    }
}

#[test]
fn minimal_roff_folding_hides_only_true_subsections_not_following_roots() {
    let content = mant_loader::load_roff_bytes(
        b".TH TREE 1\n.SH BEFORE\nIntro.\n.SH PARENT\nParent text.\n.SS ChildOne\nFirst child.\n.SS ChildTwo\nSecond child.\n.SH AFTER\nFinal text.\n",
    ).unwrap();
    let view = DocumentView::new(&content);
    let parent = section_node(&view, "PARENT");
    for title in ["ChildOne", "ChildTwo"] {
        let child = section_node(&view, title);
        assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
        assert_eq!(child.depth, 1);
    }
    let mut app = App::new(&content);
    let mut terminal = terminal(&mut app);
    let root_column = position(&terminal, "BEFORE").0;
    assert_eq!(position(&terminal, "PARENT").0, root_column);
    assert_eq!(position(&terminal, "AFTER").0, root_column);
    for title in ["ChildOne", "ChildTwo"] {
        assert!(position(&terminal, title).0 > root_column);
    }
    select_label(&mut app, &terminal, "PARENT");
    key(&mut app, KeyCode::Left);
    draw(&mut app, &mut terminal);
    for title in ["ChildOne", "ChildTwo"] {
        assert_eq!(label_position(terminal.backend().buffer(), title), None);
    }
    for title in ["BEFORE", "PARENT", "AFTER"] {
        assert_eq!(position(&terminal, title).0, root_column);
    }
    key(&mut app, KeyCode::Right);
    draw(&mut app, &mut terminal);
    for title in ["ChildOne", "ChildTwo"] {
        assert!(position(&terminal, title).0 > root_column);
    }
    assert_eq!(position(&terminal, "AFTER").0, root_column);
}
