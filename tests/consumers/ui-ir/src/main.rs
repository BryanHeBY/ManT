//! Embed the reader around authored IR without any loader or process lifecycle.
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use mant_ir::{
    Block, Document, DocumentMeta, DocumentSource, Inline, LayoutHint, ResolvedContent, Section,
    SourceFormat,
};
use mant_ui::{App, ReaderOptions, ReaderServices};
use ratatui::{Terminal, backend::TestBackend};
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
                blocks: vec![Block::Paragraph {
                    children: vec![Inline::Text {
                        value: "Embedded Cafe\u{301} 👩‍💻".into(),
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                }],
            }],
        }),
    }
}

fn check_reader() -> Result<(), Box<dyn std::error::Error>> {
    let source = Arc::new(content());
    assert!(mant_ir::validate_document(source.document.as_ref().unwrap()).is_empty());
    let mut app = App::from_shared(ReaderOptions::new(Arc::clone(&source)));
    let mut terminal = Terminal::new(TestBackend::new(160, 24))?;
    terminal.draw(|frame| app.draw(frame))?;
    let cells = terminal.backend().buffer().content();
    assert!(cells.iter().any(|cell| cell.symbol() == "e\u{301}"));
    assert!(cells.iter().any(|cell| cell.symbol() == "👩‍💻"));

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    check_reader()
}

#[test]
fn embedded_reader_uses_authored_ir_and_explicit_capabilities() {
    check_reader().unwrap();
}
