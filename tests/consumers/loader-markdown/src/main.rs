//! Independent read-only consumer of the loader's Markdown-only public API.

use mant_loader::{DocumentLoader, LoadPolicy, LoadSpec};
use mant_protocol::InputFormat;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let path = arguments.next().ok_or("expected a Markdown source path")?;
    let expected_heading = arguments.next().ok_or("expected the source heading")?;
    assert!(arguments.next().is_none());

    let loader = DocumentLoader::from_system();
    let content = loader.load(
        LoadSpec::File {
            path: &path,
            format: InputFormat::Markdown,
        },
        LoadPolicy::Combined,
    )?;
    assert!(content.tldr.is_none());
    let document = content.document.as_ref().ok_or("missing loaded document")?;
    assert_eq!(document.source.path.as_deref(), Some(path.as_str()));
    assert_eq!(
        document
            .heading
            .as_ref()
            .map(|heading| heading.plain_text()),
        Some(expected_heading)
    );
    assert!(document.diagnostics.is_empty());
    assert!(!document.blocks.is_empty());
    assert_eq!(document.sections.len(), 1);
    assert_eq!(document.sections[0].heading.plain_text(), "Options");
    Ok(())
}
