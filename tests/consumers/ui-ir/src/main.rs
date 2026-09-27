//! Embed the reader around authored IR without any loader or process lifecycle.
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use mant_ir::{
    Block, ContentOwnerKind, ContentRootKind, ContentStoreBuilder, ContentStyle, Document,
    DocumentBody, DocumentMeta, FlowBody, Heading, Inline, LayoutHint, Provenance, ResolvedContent,
    Section, SourceCoordinates, SourceFormat, SourceIdentity, SourceKey, SourceRecord,
};
use mant_ui::{App, ReaderOptions, ReaderServices};
use ratatui::{Terminal, backend::TestBackend};
use std::sync::Arc;

fn content() -> ResolvedContent {
    let mut builder = ContentStoreBuilder::new();
    let document_owner = builder.push_owner(ContentOwnerKind::Document, Provenance::Unknown);
    let document_heading_root = builder.push_root(
        document_owner,
        ContentRootKind::Heading,
        Provenance::Unknown,
    );
    let document_heading = builder.push_text(
        document_heading_root,
        "Independent reader".into(),
        None,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    let owner = builder.push_owner(ContentOwnerKind::Section, Provenance::Unknown);
    let heading_root = builder.push_root(owner, ContentRootKind::Heading, Provenance::Unknown);
    let heading = builder.push_text(
        heading_root,
        "Overview".into(),
        None,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    let body_root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
    let body = builder.push_text(
        body_root,
        "Embedded Cafe\u{301} 👩‍💻".into(),
        None,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    ResolvedContent {
        address: None,
        label: "Independent reader".into(),
        tldr: None,
        document: Some(Document {
            parser: None,
            sources: vec![SourceRecord {
                key: SourceKey::FIRST,
                identity: SourceIdentity::Path {
                    name: "does-not-exist/私有 source.md".into(),
                },
                format: SourceFormat::Markdown,
                decoded_byte_length: 0,
                content_sha256: None,
                coordinates: SourceCoordinates::DecodedUtf8Bytes,
            }],
            root_source: SourceKey::FIRST,
            meta: DocumentMeta::default(),
            fragment_aliases: vec![],
            diagnostics: vec![],
            body: DocumentBody::Flow(FlowBody {
                content_store: builder.finish(),
                heading: Some(Heading {
                    content: vec![Inline::Text {
                        content: document_heading,
                    }],
                    source: None,
                }),
                blocks: vec![],
                sections: vec![Section {
                    id: "overview".into(),
                    fragment_aliases: vec![],
                    heading: Heading {
                        content: vec![Inline::Text { content: heading }],
                        source: None,
                    },
                    spacing_before_lines: 0,
                    children: vec![],
                    source: None,
                    blocks: vec![Block::Paragraph {
                        children: vec![Inline::Text { content: body }],
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                }],
            }),
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
        source.document.as_ref().unwrap().flow().unwrap().sections[0]
            .heading
            .plain_text(source.document.as_ref().unwrap().content()),
        "Overview"
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
