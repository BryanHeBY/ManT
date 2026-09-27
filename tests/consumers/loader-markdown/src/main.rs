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
    assert_eq!(document.root_path(), Some(path.as_str()));
    let flow = document.flow().ok_or("Markdown is not a Flow document")?;
    assert_eq!(
        flow.heading
            .as_ref()
            .map(|heading| heading.plain_text(document.content())),
        Some(expected_heading)
    );
    assert!(document.diagnostics.is_empty());
    assert!(!flow.blocks.is_empty());
    assert_eq!(flow.sections.len(), 1);
    assert_eq!(
        flow.sections[0].heading.plain_text(document.content()),
        "Options"
    );
    Ok(())
}
