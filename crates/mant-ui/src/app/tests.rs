//! End-to-end interaction and rendering regression tests for the application state machine.

use std::{
    collections::HashSet,
    path::Path,
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use mant_ir::{
    Block as AstBlock, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole,
    Document, DocumentMeta, DocumentSource, Inline, LayoutHint, ResolvedContent, Section,
    SourceFormat, TldrDocument, TldrOrigin,
};
use mant_protocol::{
    CatalogSchema, DocumentAddress, DocumentCatalog, DocumentSummary, MarkdownOrigin,
};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};

use super::{
    App, COPY_TOAST_DURATION, NAVIGATION_SYNC_IDLE, Overlay, PointerDrag,
    SELECTION_AUTO_SCROLL_INTERVAL, SIDEBAR_RESIZE_FRAME_INTERVAL, UpdateOutcome,
    finder::FinderTreeRow,
    menu::{MenuAction, MenuId},
    render::sidebar_metadata,
    search::SearchMode,
};
use crate::{
    CopyFormat, CopyRequest, NavKind, RenderedSelection, TextPosition,
    layout::{CONTENT_SCROLLBAR_GAP, DEFAULT_SIDEBAR_WIDTH, SIDEBAR_SPLITTER_WIDTH},
    theme,
};

fn click_document_cell(app: &mut App, column: usize, row: usize) {
    let column = app.geometry.content.x + u16::try_from(column).expect("document column");
    let row = app.geometry.content.y
        + u16::try_from(row.saturating_sub(app.session.content_scroll)).expect("document row");
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
    }
}

fn empty_bundle() -> ResolvedContent {
    ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: None,
        tldr: None,
    }
}

fn tldr_bundle() -> ResolvedContent {
    ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: None,
        tldr: Some(TldrDocument {
            title: "demo".to_owned(),
            description: vec!["A polished quick reference".to_owned()],
            more_information: None,
            examples: Vec::new(),
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "demo.md".to_owned(),
            origin: TldrOrigin::TldrPages,
        }),
    }
}

fn navigation_bundle() -> ResolvedContent {
    let paragraph = |value: &str| AstBlock::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some(Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Man,
                path: None,
            },
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![Section {
                id: "options".to_owned().into(),
                fragment_aliases: Vec::new(),
                title: "OPTIONS".to_owned(),
                spacing_before_lines: 0,
                blocks: vec![AstBlock::DefinitionList {
                    items: vec![DefinitionItem {
                        identity: Some(DefinitionIdentity {
                            forms: Vec::new(),
                            id: "help-option".to_owned().into(),
                            role: DefinitionRole::Option,
                            case: DefinitionCase::Sensitive,
                            names: vec!["-h".to_owned(), "--help".to_owned()],
                            value_domain: None,
                        }),
                        terms: vec![vec![Inline::Text {
                            value: "-h, --help".to_owned(),
                        }]],
                        description: vec![paragraph("Show help")],
                        inline_term: false,
                        spacing_before_lines: None,
                    }],
                    compact: true,
                    layout: LayoutHint::default(),
                    source: None,
                }],
                children: vec![Section {
                    id: "details".to_owned().into(),
                    fragment_aliases: Vec::new(),
                    title: "Details".to_owned(),
                    spacing_before_lines: 0,
                    blocks: vec![paragraph("Nested details")],
                    children: Vec::new(),
                    source: None,
                }],
                source: None,
            }],
        }),
        tldr: None,
    }
}

fn reflow_navigation_bundle() -> ResolvedContent {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().expect("document").sections = (0..24)
        .map(|index| Section {
            id: format!("section-{index}").into(),
            fragment_aliases: Vec::new(),
            title: format!("Section {index}"),
            spacing_before_lines: 0,
            blocks: Vec::new(),
            children: vec![Section {
                id: format!("section-{index}-child").into(),
                fragment_aliases: Vec::new(),
                title: format!(
                    "A deliberately long nested section title before selected node {index}"
                ),
                spacing_before_lines: 0,
                blocks: Vec::new(),
                children: Vec::new(),
                source: None,
            }],
            source: None,
        })
        .collect();
    bundle
}

fn selected_navigation_viewport_row(app: &App) -> usize {
    app.geometry
        .navigation_rows
        .iter()
        .position(|index| *index == app.selected)
        .expect("selected navigation row is visible")
}

fn document_catalog() -> DocumentCatalog {
    let documents = vec![
        DocumentSummary {
            address: DocumentAddress::Markdown {
                path: "Start-Process".to_owned(),
                origin: MarkdownOrigin::Source {
                    name: "pwsh7".to_owned(),
                },
            },
        },
        DocumentSummary {
            address: DocumentAddress::Manual {
                name: "printf".to_owned(),
                manual_section: "3".to_owned(),
            },
        },
    ];
    DocumentCatalog {
        schema: CatalogSchema::V0Dot11,
        query: mant_protocol::CatalogQuery::default(),
        coverage: mant_protocol::CatalogCoverage::default(),
        total: 2,
        returned: 2,
        offset: 0,
        truncated: false,
        next_offset: None,
        documents,
    }
}

fn overflowing_document_catalog() -> DocumentCatalog {
    let documents = (0..40)
        .map(|index| {
            let name = format!("tool-{index:02}");
            DocumentSummary {
                address: DocumentAddress::Manual {
                    name: name.clone(),
                    manual_section: "1".to_owned(),
                },
            }
        })
        .collect::<Vec<_>>();
    DocumentCatalog {
        schema: CatalogSchema::V0Dot11,
        query: mant_protocol::CatalogQuery::default(),
        coverage: mant_protocol::CatalogCoverage::default(),
        total: u32::try_from(documents.len()).expect("fixture length"),
        returned: u32::try_from(documents.len()).expect("fixture length"),
        offset: 0,
        truncated: false,
        next_offset: None,
        documents,
    }
}

fn manual_bundle(name: &str, section: &str) -> ResolvedContent {
    let mut bundle = navigation_bundle();
    bundle.label = name.to_owned();
    bundle.address = Some(DocumentAddress::Manual {
        name: name.to_owned(),
        manual_section: section.to_owned(),
    });
    bundle
        .document
        .as_mut()
        .expect("manual")
        .meta
        .manual_section = Some(section.to_owned());
    bundle
}

fn open_manual(app: &mut App, name: &str, section: &str) {
    app.request_open(
        DocumentAddress::Manual {
            name: name.to_owned(),
            manual_section: section.to_owned(),
        },
        None,
    );
    let request = app.take_open_request().expect("manual open request");
    app.complete_open(&manual_bundle(name, section), request);
}

#[test]
fn document_tab_stack_evicts_the_oldest_identity_at_its_bound() {
    let mut app = App::new(&manual_bundle("initial", "1"));
    for index in 1..=64 {
        open_manual(&mut app, &format!("tool-{index}"), "1");
    }

    assert_eq!(app.document_tabs.len(), 64);
    assert_eq!(app.document_tabs[0].label, "tool-1(1)");
    assert_eq!(app.document_tabs[63].label, "tool-64(1)");
    assert_eq!(app.active_document_tab, 63);
}

#[test]
fn document_tab_width_matrix_is_terminal_safe_and_keeps_the_active_identity() {
    let mut app = App::new(&manual_bundle("编译器选项与输出格式", "1"));
    open_manual(&mut app, "a-deliberately-long-shared-prefix-alpha", "1");
    open_manual(&mut app, "a-deliberately-long-shared-prefix-omega", "1");
    open_manual(&mut app, "unsafe\u{1b}[2J-title", "1");

    for width in [44, 45, 46, 48, 60, 86, 120] {
        let backend = TestBackend::new(width, 12);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal.draw(|frame| app.draw(frame)).expect("draw tabs");
        if width == 44 {
            assert!(app.geometry.document_tabs.is_empty());
            continue;
        }
        assert!(
            app.geometry
                .document_tabs
                .iter()
                .any(|tab| tab.index == app.active_document_tab),
            "active tab is visible at width {width}"
        );
        assert!(
            app.geometry
                .document_tabs
                .iter()
                .all(|tab| tab.area.x >= 44 && tab.area.right() <= width),
            "tab geometry stays inside width {width}"
        );
        assert!(
            app.geometry
                .document_tabs
                .windows(2)
                .all(|pair| pair[0].area.right() <= pair[1].area.x),
            "tab hit regions do not overlap at width {width}"
        );
        assert!(
            !terminal.backend().to_string().contains('\u{1b}'),
            "tab labels cannot emit terminal controls"
        );
    }
}

#[test]
fn renders_the_application_chrome_in_a_test_backend() {
    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&empty_bundle());

    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let screen = terminal.backend().to_string();

    assert!(screen.contains("Manual"));
    assert!(screen.contains("OUTLINE"));
    assert!(screen.contains("MANUAL · demo"));
    assert!(screen.contains("0 visible nodes"));
}

#[test]
fn the_final_section_heading_can_become_the_first_content_row() {
    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal
        .draw(|frame| app.draw(frame))
        .expect("initial draw");
    app.set_selected_index(3);
    app.scroll_to_selected();
    let width = app.geometry.content.width;
    let expected = app.session.rendered_cache[&width]
        .anchor_row("details")
        .expect("details anchor");

    terminal
        .draw(|frame| app.draw(frame))
        .expect("scrolled draw");

    assert_eq!(app.session.content_scroll, expected);
    let row = app.geometry.content.y;
    let content = (app.geometry.content.x..app.geometry.content.right())
        .filter_map(|column| terminal.backend().buffer().cell((column, row)))
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(content.trim_start().starts_with("Details"));
}

#[test]
fn help_overlay_is_safe_on_a_tiny_terminal() {
    let backend = TestBackend::new(12, 4);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&empty_bundle());
    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));

    terminal
        .draw(|frame| app.draw(frame))
        .expect("tiny help draw");
}

#[test]
fn q_requests_a_clean_exit() {
    let mut app = App::new(&empty_bundle());
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(app.should_quit());
}

#[test]
fn right_click_in_document_content_copies_the_retained_selection() {
    let backend = TestBackend::new(80, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("Show help")
        .into_iter()
        .next()
        .expect("visible description");
    app.selection = Some(RenderedSelection {
        anchor: TextPosition {
            row: region.row,
            column: region.start_column,
        },
        focus: TextPosition {
            row: region.row,
            column: region.end_column.saturating_sub(1),
        },
    });

    let outcome = app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: app.geometry.content.x,
        row: app.geometry.content.y,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(outcome, UpdateOutcome::Redraw);
    let CopyRequest::Selection { text } = app.take_copy_request().expect("right-click copy") else {
        panic!("visual selection emitted a semantic node");
    };
    assert_eq!(text, "Show help");
    assert!(app.selection.is_some(), "copying must retain the selection");
}

#[test]
fn right_click_without_a_selection_is_inert() {
    let backend = TestBackend::new(80, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");

    let outcome = app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: app.geometry.content.x,
        row: app.geometry.content.y,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(outcome, UpdateOutcome::Unchanged);
    assert!(app.take_copy_request().is_none());
    assert!(app.notice.is_none());
}

#[test]
fn visual_tldr_copy_omits_panel_decoration() {
    let backend = TestBackend::new(80, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&tldr_bundle());
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let rendered = &app.session.rendered_cache[&app.session.content_render_width];
    let last_row = rendered.row_count.saturating_sub(1);
    app.selection = Some(RenderedSelection {
        anchor: TextPosition { row: 0, column: 0 },
        focus: TextPosition {
            row: last_row,
            column: usize::from(app.session.content_render_width.saturating_sub(1)),
        },
    });

    app.copy_selection();

    let CopyRequest::Selection { text } = app.take_copy_request().expect("copy request") else {
        panic!("visual selection emitted a semantic node");
    };
    assert!(text.contains("TLDR QUICK REFERENCE"));
    assert!(!text.contains(['│', '┌', '┐', '└', '┘', '─']));
}

#[test]
fn edit_menu_width_fits_its_longest_item() {
    let backend = TestBackend::new(80, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());
    app.open_menu(MenuId::Edit);

    terminal.draw(|frame| app.draw(frame)).expect("draw menu");

    assert!(
        terminal
            .backend()
            .to_string()
            .contains("Copy Current Node as Markdown")
    );
}

#[test]
fn question_mark_opens_and_closes_keyboard_help() {
    let backend = TestBackend::new(100, 22);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&navigation_bundle());

    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    terminal.draw(|frame| app.draw(frame)).expect("draw help");
    assert!(
        terminal
            .backend()
            .to_string()
            .contains("Keyboard Shortcuts")
    );

    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert_eq!(app.overlay, Overlay::None);
}

#[test]
fn clicking_a_manual_reference_requests_the_exact_page() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().expect("manual").sections[0]
        .blocks
        .insert(
            0,
            AstBlock::Paragraph {
                children: vec![Inline::Link {
                    target: mant_ir::LinkTarget::Manual {
                        name: "git-add".to_owned(),
                        manual_section: Some("1".to_owned()),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "git-add(1)".to_owned(),
                    }],
                }],
                layout: LayoutHint::default(),
                source: None,
            },
        );
    let backend = TestBackend::new(72, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("git-add")
        .into_iter()
        .next()
        .expect("visible manual reference");

    click_document_cell(&mut app, region.start_column, region.row);

    assert_eq!(
        app.take_open_request().expect("manual request").address(),
        &DocumentAddress::Manual {
            name: "git-add".to_owned(),
            manual_section: "1".to_owned(),
        }
    );
}

#[test]
fn clicking_a_real_git_manual_reference_requests_git_add_section_one() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/archlinux/git.1.gz");
    if !fixture.exists() {
        // The repository owns the separately licensed real-page corpus; the
        // published UI package keeps this integration test optional.
        return;
    }
    let document = mant_engine::parse_manual_source(&fixture).expect("parse real git manual");
    let bundle = ResolvedContent {
        address: Some(DocumentAddress::Manual {
            name: "git".to_owned(),
            manual_section: "1".to_owned(),
        }),
        label: "git".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let backend = TestBackend::new(132, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw git");
    let width = app.geometry.content.width;
    let matches = app.session.rendered_cache[&width].search("git-add(1)");

    let mut opened = None;
    for region in matches {
        app.session.content_scroll = region.row;
        click_document_cell(&mut app, region.start_column, region.row);
        if let Some(request) = app.take_open_request() {
            opened = Some(request.address().clone());
            break;
        }
    }

    assert_eq!(
        opened,
        Some(DocumentAddress::Manual {
            name: "git-add".to_owned(),
            manual_section: "1".to_owned(),
        })
    );
}

mod entries;
mod layout;
mod navigation;
mod search;
