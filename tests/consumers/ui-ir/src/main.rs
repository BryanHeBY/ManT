//! Embed the reader around authored IR without any loader or process lifecycle.
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use mant_ir::{
    Block, Document, DocumentMeta, DocumentSource, Inline, LayoutHint, ResolvedContent, Section,
    SourceFormat,
};
use mant_ui::{App, DocumentView, ReaderOptions, ReaderServices};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};
use std::sync::Arc;

fn content() -> ResolvedContent {
    ResolvedContent {
        address: None,
        label: "Independent reader".into(),
        tldr: None,
        document: Some(Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Markdown,
                // Provenance is never interpreted as a path to open by the UI.
                path: Some("does-not-exist/私有 source.md".into()),
            },
            meta: DocumentMeta::default(),
            heading: Some("Independent reader".into()),
            fragment_aliases: vec![],
            diagnostics: vec![],
            blocks: vec![],
            sections: vec![Section {
                id: "overview".into(),
                fragment_aliases: vec![],
                heading: "Overview".into(),
                spacing_before_lines: 0,
                children: vec![],
                source: None,
                blocks: vec![
                    Block::Paragraph {
                        inline_layout: mant_ir::InlineLayout::default(),
                        children: vec![Inline::Text {
                            value: "Embedded Cafe\u{301} 👩‍💻".into(),
                        }],
                        layout: LayoutHint::default(),
                        source: None,
                    },
                    Block::Preformatted {
                        inline_layout: mant_ir::InlineLayout::default(),
                        children: vec![Inline::Text {
                            value: "echo \"$HOME\" # note".into(),
                        }],
                        language: Some("sh".into()),
                        layout: LayoutHint::default(),
                        source: None,
                    },
                ],
            }],
        }),
    }
}

fn check_reader() -> Result<(), Box<dyn std::error::Error>> {
    let source = Arc::new(content());
    assert!(mant_ir::validate_document(source.document.as_ref().unwrap()).is_empty());
    check_hinted_owner(&source);
    let mut app = App::from_shared(ReaderOptions::new(Arc::clone(&source)));
    let mut terminal = Terminal::new(TestBackend::new(160, 24))?;
    terminal.draw(|frame| app.draw(frame))?;
    let cells = terminal.backend().buffer().content();
    assert!(cells.iter().any(|cell| cell.symbol() == "e\u{301}"));
    assert!(cells.iter().any(|cell| cell.symbol() == "👩‍💻"));
    // The standalone reader prepares lexical accents without consulting the
    // deliberately nonexistent provenance path or any host loading service.
    let foreground = |word: &str| {
        cells
            .windows(word.len())
            .find(|slice| {
                slice
                    .iter()
                    .zip(word.chars())
                    .all(|(cell, character)| cell.symbol() == character.to_string())
            })
            .expect("visible code token")[0]
            .fg
    };
    assert_ne!(foreground("HOME"), foreground("note"));

    let mut services = ReaderServices::default();
    assert!(!app.service_pending(&mut services));
    app.handle_event(&Event::Key(KeyEvent::new(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL,
    )));
    assert!(app.service_pending(&mut services));
    assert!(!app.service_pending(&mut services));
    terminal.draw(|frame| app.draw(frame))?;
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(
        text.contains("document discovery is unavailable in this host"),
        "{text}"
    );
    app.handle_event(&Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    app.handle_event(&Event::Key(KeyEvent::new(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
    )));
    assert!(app.should_quit());
    assert_eq!(
        source.document.as_ref().unwrap().sections[0].heading,
        "Overview".into()
    );
    Ok(())
}

fn check_hinted_owner(source: &ResolvedContent) {
    let mut authored = source.clone();
    let Block::Paragraph {
        children,
        inline_layout,
        ..
    } = &mut authored.document.as_mut().unwrap().sections[0].blocks[0]
    else {
        panic!("paragraph root");
    };
    *children = vec![
        Inline::Text {
            value: "First".into(),
        },
        Inline::LineBreak {},
        Inline::Strong {
            children: vec![Inline::Text {
                value: "Second".into(),
            }],
        },
    ];
    inline_layout.row_hints = vec![mant_ir::RowLayoutHint {
        row: 1,
        indent_columns: 4,
    }];
    let restored: ResolvedContent = mant_protocol::QueryBundle::from(&authored).into();
    assert_eq!(restored, authored);
    let document = restored.document.as_ref().unwrap();
    assert!(mant_ir::validate_document(document).is_empty());
    let location = mant_ir::ContentLocation::Content {
        sections: vec![0],
        blocks: vec![mant_ir::ContentBlockStep::Block { index: 0 }],
        root: mant_ir::ContentInlineRoot::Inlines,
        path: vec![2, 0],
    };
    let root = location.inline_content(document).unwrap();
    assert_eq!(mant_ir::inline_plain_text(root.content), "First\nSecond");
    assert_eq!(mant_ir::logical_row_count(root.content), 2);
    assert_eq!(root.layout.row_indent(1), 4);
    let view = DocumentView::new(&restored);
    let rendered = view.render(80);
    let first = &rendered.search("First")[0];
    let second = &rendered.search("Second")[0];
    assert_eq!(first.start_column, 3);
    assert_eq!(second.start_column, 7);
    assert_eq!(second.row, first.row + 1);
    let area = Rect::new(0, 0, 80, rendered.row_count.try_into().unwrap());
    let mut cells = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut cells);
    for (index, character) in "Second".chars().enumerate() {
        assert_eq!(
            cells[(
                u16::try_from(7 + index).unwrap(),
                u16::try_from(second.row).unwrap()
            )]
                .symbol(),
            character.to_string()
        );
    }
    assert_eq!(view.render(80).text, rendered.text);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    check_reader()
}

#[test]
fn embedded_reader_uses_authored_ir_and_explicit_capabilities() {
    check_reader().unwrap();
}
