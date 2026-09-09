use super::*;
use crate::markdown::{
    headings::{extract_document_title, nest_sections},
    source::parser_events,
};

#[test]
fn extracted_heading_keeps_original_unicode_span_and_exact_fragment_alias() {
    for newline in ["\n", "\r\n", "\r"] {
        let title = format!("# Café {{#Mixed.Target}}{newline}");
        let text =
            format!("{title}{newline}Intro.{newline}{newline}### Child{newline}Body.{newline}");
        let source = MarkdownSource::new(&text);
        let mut parsed = lower_document_structure(parser_events(&text), &source);
        assert!(parsed.diagnostics.is_empty());
        let mut sections = nest_sections(parsed.flat_sections);
        let (heading, aliases) = extract_document_title(
            &mut parsed.root_blocks,
            &mut sections,
            parsed.document_title_id.as_deref(),
        )
        .unwrap();
        assert_eq!(heading.plain_text(), "Café");
        assert_eq!(heading.source, Some(source.span(&(0..title.len()))));
        assert!(aliases.iter().any(|alias| alias.as_str() == "Mixed.Target"));
        assert!(aliases.iter().any(|alias| alias.as_str() == "café"));
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].heading.plain_text(), "Child");
        assert_eq!(parsed.root_blocks.len(), 1);
        assert_eq!(sections[0].blocks.len(), 1);
    }
}
